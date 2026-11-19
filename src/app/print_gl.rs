//! OpenGL for the 3D-Druck view: the build plate with its surface texture (drawn
//! procedurally: powder-coated PEI, smooth PEI, cool plate, glass, …), the part on it in
//! the filament's colour, the printed beads as shaded tubes and the travel moves.

use eframe::glow::{self, HasContext};
use glam::Mat4;
use std::sync::Arc;

/// Geometry of the extruded beads: four corners per path point (right, top, left, bottom).
#[derive(Default)]
pub struct Beads {
    pub pos: Vec<f32>,
    /// normal xyz as signed bytes (w unused)
    pub nrm: Vec<i8>,
    pub idx: Vec<u32>,
    /// move index of every path point (4 vertices each), for colouring
    pub point_move: Vec<u32>,
    /// number of indices emitted before move m (len = moves + 1)
    pub idx_before: Vec<u32>,
    /// travel lines: pos(3) per vertex, two per move
    pub travel: Vec<f32>,
    pub travel_move: Vec<u32>,
    pub travel_before: Vec<u32>,
}

/// Everything static of a scene; `gen` changes when it must be uploaded again.
#[derive(Default)]
pub struct Scene {
    pub gen: u64,
    /// bed top: pos(3) bed-mm(2)
    pub bed: Vec<f32>,
    /// lit triangles: pos(3) normal(3) colour(3) (bed slab, part)
    pub solid: Vec<f32>,
    pub beads: Arc<Beads>,
}

pub struct Colors {
    pub gen: u64,
    /// rgba per bead vertex
    pub bead: Vec<u8>,
    /// rgba per travel vertex
    pub travel: Vec<u8>,
}

pub struct Frame {
    pub mvp: Mat4,
    pub view: Mat4,
    pub scene: Arc<Scene>,
    pub colors: Arc<Colors>,
    /// plate surface kind (0..5), bed size, round bed
    pub plate: i32,
    pub bed: [f32; 2],
    pub round: bool,
    /// draw the lit triangles (part) with this alpha (bed slab always opaque)
    pub part_alpha: f32,
    /// part triangles start at this vertex of `solid` (before: bed slab)
    pub part_from: usize,
    pub beads: (u32, u32),
    pub travel: (u32, u32),
    /// pos(3) rgba(4) lines: build volume, outlines
    pub lines: Vec<f32>,
}

struct Buf {
    vao: glow::VertexArray,
    bufs: Vec<glow::Buffer>,
    count: i32,
}

pub struct PlateRenderer {
    bed_prog: glow::Program,
    lit_prog: glow::Program,
    bead_prog: glow::Program,
    flat_prog: glow::Program,
    gen: u64,
    color_gen: u64,
    bed: Option<Buf>,
    solid: Option<Buf>,
    beads: Option<Buf>,
    bead_col: Option<glow::Buffer>,
    ebo: Option<glow::Buffer>,
    travel: Option<Buf>,
    travel_col: Option<glow::Buffer>,
    lines: Option<(glow::VertexArray, glow::Buffer)>,
}

const BED_VS: &str = r#"
in vec3 a_pos; in vec2 a_uv;
uniform mat4 u_mvp; uniform mat4 u_view;
out vec2 v_uv; out vec3 v_vpos;
void main() {
    v_uv = a_uv;
    v_vpos = (u_view * vec4(a_pos, 1.0)).xyz;
    gl_Position = u_mvp * vec4(a_pos, 1.0);
}"#;

const BED_FS: &str = r#"
in vec2 v_uv; in vec3 v_vpos;
uniform int u_plate; uniform vec2 u_size; uniform int u_round; uniform mat4 u_view;
out vec4 frag;
float hash(vec2 p) { p = fract(p * vec2(443.897, 441.423)); p += dot(p, p.yx + 19.19); return fract((p.x + p.y) * p.x); }
float noise(vec2 p) {
    vec2 i = floor(p); vec2 f = fract(p); vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}
float fbm(vec2 p) { float s = 0.0; float a = 0.5; for (int k = 0; k < 4; k++) { s += a * noise(p); p *= 2.03; a *= 0.5; } return s; }
float grid(vec2 uv, float step, float w) {
    vec2 g = abs(fract(uv / step - 0.5) - 0.5) / fwidth(uv / step);
    return 1.0 - clamp(min(g.x, g.y) - w, 0.0, 1.0);
}
void main() {
    if (u_round == 1 && length(v_uv - u_size * 0.5) > u_size.x * 0.5) discard;
    vec3 base; float speck; float spec_pow; float spec_k; float grid_k;
    if (u_plate == 0) {        // textured PEI: bronze powder coat
        float g = fbm(v_uv * 3.1);
        float s = hash(floor(v_uv * 9.0));
        base = mix(vec3(0.52, 0.38, 0.17), vec3(0.74, 0.57, 0.28), g);
        base *= 0.86 + 0.28 * s;
        speck = step(0.985, hash(floor(v_uv * 14.0) + 7.0));
        base += vec3(0.30, 0.25, 0.14) * speck;
        spec_pow = 18.0; spec_k = 0.22; grid_k = 0.10;
    } else if (u_plate == 1) { // smooth PEI: golden, glossy
        float g = fbm(v_uv * 0.12);
        base = mix(vec3(0.70, 0.53, 0.22), vec3(0.80, 0.62, 0.28), g);
        spec_pow = 70.0; spec_k = 0.45; grid_k = 0.10;
    } else if (u_plate == 2) { // cool plate: dark, fine grain
        float g = fbm(v_uv * 5.0);
        base = vec3(0.10, 0.105, 0.115) + vec3(0.05) * g;
        spec_pow = 40.0; spec_k = 0.25; grid_k = 0.18;
    } else if (u_plate == 3) { // engineering plate: grey
        float g = fbm(v_uv * 4.0);
        base = vec3(0.24, 0.25, 0.27) + vec3(0.06) * g;
        spec_pow = 30.0; spec_k = 0.25; grid_k = 0.16;
    } else if (u_plate == 4) { // glass
        float g = fbm(v_uv * 0.05);
        base = mix(vec3(0.62, 0.70, 0.72), vec3(0.72, 0.80, 0.82), g);
        spec_pow = 120.0; spec_k = 0.7; grid_k = 0.12;
    } else {                   // BuildTak: black, textured
        float g = fbm(v_uv * 6.0);
        base = vec3(0.06) + vec3(0.07) * g;
        spec_pow = 12.0; spec_k = 0.12; grid_k = 0.2;
    }
    // light from above and the viewer
    vec3 n = normalize(mat3(u_view) * vec3(0.0, 1.0, 0.0));
    vec3 l = normalize(mat3(u_view) * normalize(vec3(-0.3, 1.0, 0.45)));
    vec3 v = normalize(-v_vpos);
    vec3 h = normalize(l + v);
    float d = max(dot(n, l), 0.0);
    float s = pow(max(dot(n, h), 0.0), spec_pow) * spec_k;
    vec3 c = base * (0.55 + 0.5 * d) + vec3(s);
    // 10 mm grid and 50 mm lines, as printed on most plates
    float g1 = grid(v_uv, 10.0, 0.0);
    float g2 = grid(v_uv, 50.0, 0.4);
    vec3 line = (u_plate == 2 || u_plate == 3 || u_plate == 5) ? vec3(0.85) : vec3(0.18, 0.13, 0.06);
    c = mix(c, line, g1 * grid_k + g2 * grid_k * 1.4);
    // border
    vec2 e = min(v_uv, u_size - v_uv);
    float edge = (u_round == 1) ? (u_size.x * 0.5 - length(v_uv - u_size * 0.5)) : min(e.x, e.y);
    c *= 0.82 + 0.18 * smoothstep(0.0, 3.0, edge);
    frag = vec4(c, 1.0);
}"#;

const LIT_VS: &str = r#"
in vec3 a_pos; in vec3 a_nrm; in vec3 a_col;
uniform mat4 u_mvp; uniform mat4 u_view;
out vec3 v_nrm; out vec3 v_col; out vec3 v_vpos;
void main() {
    v_nrm = mat3(u_view) * a_nrm; v_col = a_col;
    v_vpos = (u_view * vec4(a_pos, 1.0)).xyz;
    gl_Position = u_mvp * vec4(a_pos, 1.0);
}"#;

const LIT_FS: &str = r#"
in vec3 v_nrm; in vec3 v_col; in vec3 v_vpos;
uniform float u_alpha; uniform mat4 u_view;
out vec4 frag;
void main() {
    vec3 n = normalize(v_nrm);
    if (!gl_FrontFacing) n = -n;
    vec3 l1 = normalize(mat3(u_view) * normalize(vec3(-0.35, 1.0, 0.55)));
    vec3 l2 = normalize(mat3(u_view) * normalize(vec3(0.6, 0.3, -0.7)));
    vec3 v = normalize(-v_vpos);
    float d = max(dot(n, l1), 0.0) * 0.62 + max(dot(n, l2), 0.0) * 0.22;
    float s = pow(max(dot(n, normalize(l1 + v)), 0.0), 40.0) * 0.28;
    vec3 up = mat3(u_view) * vec3(0.0, 1.0, 0.0);
    float hemi = 0.5 + 0.5 * dot(n, normalize(up));
    vec3 c = v_col * (0.28 + 0.14 * hemi + d) + vec3(s);
    frag = vec4(c, u_alpha);
}"#;

const BEAD_VS: &str = r#"
in vec3 a_pos; in vec4 a_nrm; in vec4 a_col;
uniform mat4 u_mvp; uniform mat4 u_view;
out vec3 v_nrm; out vec4 v_col; out vec3 v_vpos;
void main() {
    v_nrm = mat3(u_view) * a_nrm.xyz; v_col = a_col;
    v_vpos = (u_view * vec4(a_pos, 1.0)).xyz;
    gl_Position = u_mvp * vec4(a_pos, 1.0);
}"#;

const BEAD_FS: &str = r#"
in vec3 v_nrm; in vec4 v_col; in vec3 v_vpos;
uniform mat4 u_view;
out vec4 frag;
void main() {
    vec3 n = normalize(v_nrm);
    if (!gl_FrontFacing) n = -n;
    vec3 l1 = normalize(mat3(u_view) * normalize(vec3(-0.35, 1.0, 0.55)));
    vec3 l2 = normalize(mat3(u_view) * normalize(vec3(0.6, 0.35, -0.7)));
    vec3 v = normalize(-v_vpos);
    vec3 up = normalize(mat3(u_view) * vec3(0.0, 1.0, 0.0));
    float hemi = 0.5 + 0.5 * dot(n, up);
    float d = max(dot(n, l1), 0.0) * 0.6 + max(dot(n, l2), 0.0) * 0.2;
    // plastic: soft sheen, a little rim light so the beads read as round
    float s = pow(max(dot(n, normalize(l1 + v)), 0.0), 36.0) * 0.30;
    float rim = pow(1.0 - max(dot(n, v), 0.0), 3.0) * 0.10;
    vec3 c = v_col.rgb * (0.22 + 0.22 * hemi + d) + vec3(s + rim);
    frag = vec4(c, 1.0);
}"#;

const FLAT_VS: &str = r#"
in vec3 a_pos; in vec4 a_col;
uniform mat4 u_mvp;
out vec4 v_col;
void main() { v_col = a_col; gl_Position = u_mvp * vec4(a_pos, 1.0); gl_Position.z -= 0.0004 * gl_Position.w; }"#;

const FLAT_FS: &str = r#"
in vec4 v_col;
out vec4 frag;
void main() { frag = v_col; }"#;

unsafe fn program(gl: &glow::Context, vs: &str, fs: &str, attrs: &[&str]) -> glow::Program {
    let header = "#version 150\n";
    let p = gl.create_program().expect("program");
    let mut shaders = vec![];
    for (ty, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
        let s = gl.create_shader(ty).expect("shader");
        gl.shader_source(s, &format!("{}{}", header, src));
        gl.compile_shader(s);
        if !gl.get_shader_compile_status(s) {
            panic!("shader compile error: {}", gl.get_shader_info_log(s));
        }
        gl.attach_shader(p, s);
        shaders.push(s);
    }
    for (i, a) in attrs.iter().enumerate() {
        gl.bind_attrib_location(p, i as u32, a);
    }
    gl.bind_frag_data_location(p, 0, "frag");
    gl.link_program(p);
    if !gl.get_program_link_status(p) {
        panic!("link error: {}", gl.get_program_info_log(p));
    }
    for s in shaders {
        gl.detach_shader(p, s);
        gl.delete_shader(s);
    }
    p
}

fn bytes<T>(d: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(d.as_ptr() as *const u8, std::mem::size_of_val(d)) }
}

unsafe fn float_buf(gl: &glow::Context, data: &[f32], layout: &[i32]) -> Buf {
    let vao = gl.create_vertex_array().unwrap();
    let vbo = gl.create_buffer().unwrap();
    gl.bind_vertex_array(Some(vao));
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(data), glow::STATIC_DRAW);
    let stride: i32 = layout.iter().sum();
    let mut off = 0;
    for (i, &n) in layout.iter().enumerate() {
        gl.enable_vertex_attrib_array(i as u32);
        gl.vertex_attrib_pointer_f32(i as u32, n, glow::FLOAT, false, stride * 4, off * 4);
        off += n;
    }
    gl.bind_vertex_array(None);
    Buf { vao, bufs: vec![vbo], count: data.len() as i32 / stride.max(1) }
}

unsafe fn free(gl: &glow::Context, b: Option<Buf>) {
    if let Some(b) = b {
        gl.delete_vertex_array(b.vao);
        for x in b.bufs {
            gl.delete_buffer(x);
        }
    }
}

impl PlateRenderer {
    pub fn new(gl: &glow::Context) -> PlateRenderer {
        unsafe {
            PlateRenderer {
                bed_prog: program(gl, BED_VS, BED_FS, &["a_pos", "a_uv"]),
                lit_prog: program(gl, LIT_VS, LIT_FS, &["a_pos", "a_nrm", "a_col"]),
                bead_prog: program(gl, BEAD_VS, BEAD_FS, &["a_pos", "a_nrm", "a_col"]),
                flat_prog: program(gl, FLAT_VS, FLAT_FS, &["a_pos", "a_col"]),
                gen: u64::MAX,
                color_gen: u64::MAX,
                bed: None,
                solid: None,
                beads: None,
                bead_col: None,
                ebo: None,
                travel: None,
                travel_col: None,
                lines: None,
            }
        }
    }

    unsafe fn upload(&mut self, gl: &glow::Context, s: &Scene) {
        free(gl, self.bed.take());
        free(gl, self.solid.take());
        free(gl, self.beads.take());
        free(gl, self.travel.take());
        for b in [self.bead_col.take(), self.ebo.take(), self.travel_col.take()].into_iter().flatten() {
            gl.delete_buffer(b);
        }
        if !s.bed.is_empty() {
            self.bed = Some(float_buf(gl, &s.bed, &[3, 2]));
        }
        if !s.solid.is_empty() {
            self.solid = Some(float_buf(gl, &s.solid, &[3, 3, 3]));
        }
        let b = &s.beads;
        if !b.idx.is_empty() {
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            let pos = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(pos));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(&b.pos), glow::STATIC_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 12, 0);
            let nrm = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(nrm));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(&b.nrm), glow::STATIC_DRAW);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 4, glow::BYTE, true, 4, 0);
            let col = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(col));
            gl.buffer_data_size(glow::ARRAY_BUFFER, (b.pos.len() / 3 * 4) as i32, glow::DYNAMIC_DRAW);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 4, glow::UNSIGNED_BYTE, true, 4, 0);
            let ebo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes(&b.idx), glow::STATIC_DRAW);
            gl.bind_vertex_array(None);
            self.beads = Some(Buf { vao, bufs: vec![pos, nrm], count: b.idx.len() as i32 });
            self.bead_col = Some(col);
            self.ebo = Some(ebo);
        }
        if !b.travel.is_empty() {
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            let pos = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(pos));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(&b.travel), glow::STATIC_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 12, 0);
            let col = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(col));
            gl.buffer_data_size(glow::ARRAY_BUFFER, (b.travel.len() / 3 * 4) as i32, glow::DYNAMIC_DRAW);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 4, glow::UNSIGNED_BYTE, true, 4, 0);
            gl.bind_vertex_array(None);
            self.travel = Some(Buf { vao, bufs: vec![pos], count: (b.travel.len() / 3) as i32 });
            self.travel_col = Some(col);
        }
        self.gen = s.gen;
        self.color_gen = u64::MAX;
    }

    pub fn paint(&mut self, gl: &glow::Context, f: &Frame) {
        unsafe {
            if self.gen != f.scene.gen {
                self.upload(gl, &f.scene);
            }
            if self.color_gen != f.colors.gen {
                if let Some(c) = self.bead_col {
                    if f.colors.bead.len() == f.scene.beads.pos.len() / 3 * 4 {
                        gl.bind_buffer(glow::ARRAY_BUFFER, Some(c));
                        gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, &f.colors.bead);
                    }
                }
                if let Some(c) = self.travel_col {
                    if f.colors.travel.len() == f.scene.beads.travel.len() / 3 * 4 {
                        gl.bind_buffer(glow::ARRAY_BUFFER, Some(c));
                        gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, &f.colors.travel);
                    }
                }
                self.color_gen = f.colors.gen;
            }
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.depth_mask(true);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            let mvp = f.mvp.to_cols_array();
            let view = f.view.to_cols_array();
            let set = |p: glow::Program| {
                gl.use_program(Some(p));
                gl.uniform_matrix_4_f32_slice(gl.get_uniform_location(p, "u_mvp").as_ref(), false, &mvp);
                if let Some(l) = gl.get_uniform_location(p, "u_view") {
                    gl.uniform_matrix_4_f32_slice(Some(&l), false, &view);
                }
            };
            // bed surface (pushed back a little so prints sit on it cleanly)
            if let Some(b) = &self.bed {
                set(self.bed_prog);
                gl.uniform_1_i32(gl.get_uniform_location(self.bed_prog, "u_plate").as_ref(), f.plate);
                gl.uniform_2_f32(gl.get_uniform_location(self.bed_prog, "u_size").as_ref(), f.bed[0], f.bed[1]);
                gl.uniform_1_i32(gl.get_uniform_location(self.bed_prog, "u_round").as_ref(), f.round as i32);
                gl.enable(glow::POLYGON_OFFSET_FILL);
                gl.polygon_offset(2.0, 2.0);
                gl.bind_vertex_array(Some(b.vao));
                gl.draw_arrays(glow::TRIANGLES, 0, b.count);
                gl.disable(glow::POLYGON_OFFSET_FILL);
            }
            // bed slab (opaque) and part
            if let Some(b) = &self.solid {
                set(self.lit_prog);
                gl.bind_vertex_array(Some(b.vao));
                let split = (f.part_from as i32).min(b.count);
                gl.uniform_1_f32(gl.get_uniform_location(self.lit_prog, "u_alpha").as_ref(), 1.0);
                if split > 0 {
                    gl.draw_arrays(glow::TRIANGLES, 0, split);
                }
                if f.part_alpha > 0.0 && b.count > split {
                    gl.uniform_1_f32(gl.get_uniform_location(self.lit_prog, "u_alpha").as_ref(), f.part_alpha);
                    if f.part_alpha < 0.999 {
                        gl.depth_mask(false);
                    }
                    gl.draw_arrays(glow::TRIANGLES, split, b.count - split);
                    gl.depth_mask(true);
                }
            }
            // beads
            if let Some(b) = &self.beads {
                let (s, n) = f.beads;
                let n = n.min((b.count as u32).saturating_sub(s));
                if n > 0 {
                    set(self.bead_prog);
                    gl.bind_vertex_array(Some(b.vao));
                    gl.draw_elements(glow::TRIANGLES, n as i32, glow::UNSIGNED_INT, (s as i32) * 4);
                }
            }
            // travel
            if let Some(b) = &self.travel {
                let (s, n) = f.travel;
                let n = n.min((b.count as u32).saturating_sub(s));
                if n > 0 {
                    set(self.flat_prog);
                    gl.bind_vertex_array(Some(b.vao));
                    gl.draw_arrays(glow::LINES, s as i32, n as i32);
                }
            }
            // overlay lines
            if !f.lines.is_empty() {
                set(self.flat_prog);
                if self.lines.is_none() {
                    let vao = gl.create_vertex_array().unwrap();
                    let vbo = gl.create_buffer().unwrap();
                    gl.bind_vertex_array(Some(vao));
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                    gl.enable_vertex_attrib_array(0);
                    gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 28, 0);
                    gl.enable_vertex_attrib_array(1);
                    gl.vertex_attrib_pointer_f32(1, 4, glow::FLOAT, false, 28, 12);
                    self.lines = Some((vao, vbo));
                }
                let (vao, vbo) = self.lines.unwrap();
                gl.bind_vertex_array(Some(vao));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(&f.lines), glow::STREAM_DRAW);
                gl.draw_arrays(glow::LINES, 0, (f.lines.len() / 7) as i32);
            }
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::DEPTH_TEST);
        }
    }
}
