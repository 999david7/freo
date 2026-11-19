//! OpenGL for the CNC view: the stock as a height texture on a displaced grid with per-pixel
//! normals (tool marks show), shaded as wood, plastic or metal; the machine table; the part
//! as a ghost; the spinning cutter with its holder; and the toolpaths.

use eframe::glow::{self, HasContext};
use glam::Mat4;
use std::sync::Arc;

/// Static geometry: grid of the stock and lit triangles (table, part ghost).
#[derive(Default)]
pub struct Scene {
    pub gen: u64,
    /// grid vertices: uv(2) side(1: 0 top, 1 wall bottom, 2 wall top) normal(3)
    pub grid: Vec<f32>,
    pub grid_idx: Vec<u32>,
    /// lit triangles pos(3) nrm(3) col(3): table first, then the part from `part_from`
    pub solid: Vec<f32>,
    pub part_from: usize,
}

/// The stock surface: heights per cell (row-major, nx × ny), changes while cutting.
pub struct Heights {
    pub gen: u64,
    pub nx: usize,
    pub ny: usize,
    pub h: Arc<Vec<f32>>,
    /// protected part height per cell (deviation colouring)
    pub part: Arc<Vec<f32>>,
}

pub struct Frame {
    pub mvp: Mat4,
    pub view: Mat4,
    pub scene: Arc<Scene>,
    pub heights: Arc<Heights>,
    /// stock origin and size (WCS x, y), floor and top heights
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub floor: f32,
    pub top: f32,
    pub look: i32,
    pub color: [f32; 3],
    pub deviation: bool,
    pub part_alpha: f32,
    /// lit triangles drawn this frame (the tool) pos nrm col
    pub dynamic: Vec<f32>,
    /// pos(3) rgba(4)
    pub lines: Vec<f32>,
}

const STOCK_VS: &str = r#"
in vec2 a_uv; in float a_side; in vec3 a_nrm;
uniform mat4 u_mvp; uniform mat4 u_view; uniform sampler2D u_h;
uniform vec2 u_origin; uniform vec2 u_size; uniform float u_floor;
out vec2 v_uv; out float v_side; out vec3 v_wpos; out vec3 v_vpos; out vec3 v_nrm;
void main() {
    // side: 0 top grid, 1 bottom edge of the walls, 2 top edge of the walls
    float h = (a_side > 0.5 && a_side < 1.5) ? u_floor : texture(u_h, a_uv).r;
    vec2 xy = u_origin + a_uv * u_size;
    vec3 w = vec3(xy.x, h, -xy.y);
    v_uv = a_uv; v_side = a_side; v_wpos = vec3(xy, h);
    v_vpos = (u_view * vec4(w, 1.0)).xyz;
    v_nrm = mat3(u_view) * a_nrm;
    gl_Position = u_mvp * vec4(w, 1.0);
}"#;

const STOCK_FS: &str = r#"
in vec2 v_uv; in float v_side; in vec3 v_wpos; in vec3 v_vpos; in vec3 v_nrm;
uniform sampler2D u_h; uniform sampler2D u_part;
uniform mat4 u_view; uniform vec2 u_size; uniform vec2 u_texel; uniform float u_top;
uniform int u_look; uniform vec3 u_color; uniform int u_dev;
out vec4 frag;
float hash(vec3 p) { p = fract(p * 0.3183099 + 0.1); p *= 17.0; return fract(p.x * p.y * p.z * (p.x + p.y + p.z)); }
float noise(vec3 x) {
    vec3 i = floor(x); vec3 f = fract(x); f = f * f * (3.0 - 2.0 * f);
    return mix(mix(mix(hash(i), hash(i + vec3(1,0,0)), f.x), mix(hash(i + vec3(0,1,0)), hash(i + vec3(1,1,0)), f.x), f.y),
               mix(mix(hash(i + vec3(0,0,1)), hash(i + vec3(1,0,1)), f.x), mix(hash(i + vec3(0,1,1)), hash(i + vec3(1,1,1)), f.x), f.y), f.z);
}
void main() {
    vec3 n;
    float h = v_wpos.z;
    if (v_side < 0.5) {
        // per-pixel normal from the height texture: every tool mark catches the light
        float hl = texture(u_h, v_uv - vec2(u_texel.x, 0.0)).r;
        float hr = texture(u_h, v_uv + vec2(u_texel.x, 0.0)).r;
        float hd = texture(u_h, v_uv - vec2(0.0, u_texel.y)).r;
        float hu = texture(u_h, v_uv + vec2(0.0, u_texel.y)).r;
        vec2 cell = u_size * u_texel;
        vec2 g = vec2((hr - hl) / (2.0 * cell.x), (hu - hd) / (2.0 * cell.y));
        // on steep walls the cells form stairs, and the two-cell slope jumps between x and y
        // from column to column (stripes on round walls): take the wall's direction from a
        // 5 x 5 neighbourhood, so it turns smoothly around holes and rounds
        // how steep the drawn triangle really is (a pixel between two cells of equal height
        // on a wall must not light up as if it were flat)
        vec3 fnrm = normalize(cross(dFdx(v_wpos), dFdy(v_wpos)));
        if (fnrm.z < 0.0) fnrm = -fnrm;
        float geo = sqrt(max(1.0 - fnrm.z * fnrm.z, 0.0)) / max(fnrm.z, 1e-3);
        if (length(g) > 1.5 || geo > 1.5) {
            vec2 w = vec2(0.0);
            for (int k = -2; k <= 2; k++) {
                float fk = float(k);
                w.x += texture(u_h, v_uv + vec2(2.0 * u_texel.x, fk * u_texel.y)).r - texture(u_h, v_uv + vec2(-2.0 * u_texel.x, fk * u_texel.y)).r;
                w.y += texture(u_h, v_uv + vec2(fk * u_texel.x, 2.0 * u_texel.y)).r - texture(u_h, v_uv + vec2(fk * u_texel.x, -2.0 * u_texel.y)).r;
            }
            w = w / vec2(cell.x, cell.y);
            float steep = max(length(g), geo);
            if (length(w) > 1e-6) {
                g = normalize(w) * steep;
            } else if (length(fnrm.xy) > 1e-6) {
                g = -normalize(fnrm.xy) * steep;
            }
        }
        vec3 nw = normalize(vec3(-g.x, -g.y, 1.0));
        n = normalize(mat3(u_view) * vec3(nw.x, nw.z, -nw.y));
    } else {
        n = normalize(v_nrm);
    }
    vec3 l1 = normalize(mat3(u_view) * normalize(vec3(-0.4, 1.0, 0.6)));
    vec3 l2 = normalize(mat3(u_view) * normalize(vec3(0.7, 0.4, -0.5)));
    vec3 v = normalize(-v_vpos);
    vec3 up = normalize(mat3(u_view) * vec3(0.0, 1.0, 0.0));
    float d = max(dot(n, l1), 0.0) * 0.62 + max(dot(n, l2), 0.0) * 0.22;
    float hemi = 0.5 + 0.5 * dot(n, up);
    bool raw = v_side < 0.5 && h > u_top - 0.0005;
    vec3 base = u_color;
    float spec_pow = 30.0; float spec_k = 0.15; float env_k = 0.0;
    vec3 p = vec3(v_wpos.xy, h);
    if (u_look == 0) {
        // wood: growth rings through the block, fine pores
        float warp = noise(p * vec3(0.05, 0.4, 0.4)) * 6.0;
        float rings = fract((length(p.yz * vec2(0.9, 1.6)) + warp) * 0.55);
        float ring = smoothstep(0.0, 0.25, rings) * (1.0 - smoothstep(0.55, 1.0, rings));
        base = mix(u_color * 0.72, u_color * 1.08, ring);
        base *= 0.92 + 0.16 * noise(p * vec3(0.6, 9.0, 9.0));
        spec_pow = 12.0; spec_k = raw ? 0.03 : 0.07;
    } else if (u_look == 1) {
        spec_pow = 60.0; spec_k = 0.35;
        base *= 0.97 + 0.06 * noise(p * 2.0);
    } else {
        // metal: bright machined faces, a duller rolled skin on the untouched top
        spec_pow = raw ? 18.0 : 90.0; spec_k = raw ? 0.25 : 0.85; env_k = raw ? 0.15 : 0.45;
        if (raw) base *= 0.82 + 0.1 * noise(vec3(p.x * 0.4, p.y * 30.0, 0.0));
    }
    vec3 hv = normalize(l1 + v);
    float s = pow(max(dot(n, hv), 0.0), spec_pow) * spec_k;
    vec3 r = reflect(-v, n);
    vec3 env = mix(vec3(0.32, 0.35, 0.40), vec3(0.95, 0.97, 1.0), smoothstep(-0.3, 0.6, dot(r, up)));
    vec3 c = base * (0.25 + 0.2 * hemi + d);
    c = mix(c, env * base * 1.1, env_k) + vec3(s);
    if (u_dev == 1 && v_side < 0.5) {
        float ph = texture(u_part, v_uv).r;
        if (ph > -1e20) {
            // on the steep sides between two cells the drawn surface runs from the high cell
            // to the low one: compare with the lowest part height around the pixel, so only
            // a real cut into the part shows red
            float pmin = ph;
            for (int k = 0; k < 4; k++) {
                vec2 o = k == 0 ? vec2(u_texel.x, 0.0) : (k == 1 ? vec2(-u_texel.x, 0.0) : (k == 2 ? vec2(0.0, u_texel.y) : vec2(0.0, -u_texel.y)));
                float q = texture(u_part, v_uv + o).r;
                pmin = min(pmin, q > -1e20 ? q : -1e30);
            }
            float dv = h - ph;
            if (dv < -0.01 && h >= pmin - 0.01) {
                dv = 0.0;
            }
            vec3 dc = dv < -0.01 ? vec3(0.95, 0.12, 0.08) : (dv < 0.03 ? vec3(0.20, 0.80, 0.30) : mix(vec3(0.25, 0.55, 0.95), vec3(0.15, 0.20, 0.75), clamp(dv / 2.0, 0.0, 1.0)));
            c = dc * (0.45 + 0.6 * d) + vec3(s * 0.4);
        }
    }
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
uniform float u_alpha; uniform mat4 u_view; uniform float u_shine;
out vec4 frag;
void main() {
    vec3 n = normalize(v_nrm);
    if (!gl_FrontFacing) n = -n;
    vec3 l1 = normalize(mat3(u_view) * normalize(vec3(-0.4, 1.0, 0.6)));
    vec3 l2 = normalize(mat3(u_view) * normalize(vec3(0.7, 0.4, -0.5)));
    vec3 v = normalize(-v_vpos);
    vec3 up = normalize(mat3(u_view) * vec3(0.0, 1.0, 0.0));
    float d = max(dot(n, l1), 0.0) * 0.62 + max(dot(n, l2), 0.0) * 0.22;
    float s = pow(max(dot(n, normalize(l1 + v)), 0.0), 60.0) * u_shine;
    vec3 r = reflect(-v, n);
    vec3 env = mix(vec3(0.30, 0.33, 0.38), vec3(0.95, 0.97, 1.0), smoothstep(-0.3, 0.6, dot(r, up)));
    vec3 c = v_col * (0.25 + 0.2 * (0.5 + 0.5 * dot(n, up)) + d);
    c = mix(c, env * v_col, u_shine * 0.5) + vec3(s);
    frag = vec4(c, u_alpha);
}"#;

const FLAT_VS: &str = r#"
in vec3 a_pos; in vec4 a_col;
uniform mat4 u_mvp;
out vec4 v_col;
void main() { v_col = a_col; gl_Position = u_mvp * vec4(a_pos, 1.0); gl_Position.z -= 0.0006 * gl_Position.w; }"#;

const FLAT_FS: &str = r#"
in vec4 v_col;
out vec4 frag;
void main() { frag = v_col; }"#;

unsafe fn program(gl: &glow::Context, vs: &str, fs: &str, attrs: &[&str]) -> glow::Program {
    let p = gl.create_program().expect("program");
    let mut shaders = vec![];
    for (ty, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
        let s = gl.create_shader(ty).expect("shader");
        gl.shader_source(s, &format!("#version 150\n{}", src));
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

struct Buf {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: Option<glow::Buffer>,
    count: i32,
}

unsafe fn float_buf(gl: &glow::Context, data: &[f32], layout: &[i32], idx: Option<&[u32]>) -> Buf {
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
    let mut ebo = None;
    let mut count = data.len() as i32 / stride.max(1);
    if let Some(ix) = idx {
        let e = gl.create_buffer().unwrap();
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(e));
        gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes(ix), glow::STATIC_DRAW);
        ebo = Some(e);
        count = ix.len() as i32;
    }
    gl.bind_vertex_array(None);
    Buf { vao, vbo, ebo, count }
}

unsafe fn free(gl: &glow::Context, b: Option<Buf>) {
    if let Some(b) = b {
        gl.delete_vertex_array(b.vao);
        gl.delete_buffer(b.vbo);
        if let Some(e) = b.ebo {
            gl.delete_buffer(e);
        }
    }
}

pub struct CncRenderer {
    stock: glow::Program,
    lit: glow::Program,
    flat: glow::Program,
    gen: u64,
    hgen: u64,
    grid: Option<Buf>,
    solid: Option<Buf>,
    tex_h: Option<glow::Texture>,
    tex_p: Option<glow::Texture>,
    tex_size: (usize, usize),
    scratch: Option<(glow::VertexArray, glow::Buffer)>,
    scratch_lit: Option<(glow::VertexArray, glow::Buffer)>,
}

unsafe fn dynamic(gl: &glow::Context, slot: &mut Option<(glow::VertexArray, glow::Buffer)>, data: &[f32], layout: &[i32]) -> i32 {
    if slot.is_none() {
        let vao = gl.create_vertex_array().unwrap();
        let vbo = gl.create_buffer().unwrap();
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        let stride: i32 = layout.iter().sum();
        let mut off = 0;
        for (i, &n) in layout.iter().enumerate() {
            gl.enable_vertex_attrib_array(i as u32);
            gl.vertex_attrib_pointer_f32(i as u32, n, glow::FLOAT, false, stride * 4, off * 4);
            off += n;
        }
        *slot = Some((vao, vbo));
    }
    let (vao, vbo) = slot.unwrap();
    gl.bind_vertex_array(Some(vao));
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes(data), glow::STREAM_DRAW);
    data.len() as i32 / layout.iter().sum::<i32>()
}

unsafe fn float_tex(gl: &glow::Context, nx: usize, ny: usize, data: &[f32]) -> glow::Texture {
    let t = gl.create_texture().unwrap();
    gl.bind_texture(glow::TEXTURE_2D, Some(t));
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
    gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::R32F as i32, nx as i32, ny as i32, 0, glow::RED, glow::FLOAT, glow::PixelUnpackData::Slice(Some(bytes(data))));
    t
}

impl CncRenderer {
    pub fn new(gl: &glow::Context) -> CncRenderer {
        unsafe {
            CncRenderer {
                stock: program(gl, STOCK_VS, STOCK_FS, &["a_uv", "a_side", "a_nrm"]),
                lit: program(gl, LIT_VS, LIT_FS, &["a_pos", "a_nrm", "a_col"]),
                flat: program(gl, FLAT_VS, FLAT_FS, &["a_pos", "a_col"]),
                gen: u64::MAX,
                hgen: u64::MAX,
                grid: None,
                solid: None,
                tex_h: None,
                tex_p: None,
                tex_size: (0, 0),
                scratch: None,
                scratch_lit: None,
            }
        }
    }

    pub fn paint(&mut self, gl: &glow::Context, f: &Frame) {
        unsafe {
            if self.gen != f.scene.gen {
                free(gl, self.grid.take());
                free(gl, self.solid.take());
                if !f.scene.grid.is_empty() {
                    self.grid = Some(float_buf(gl, &f.scene.grid, &[2, 1, 3], Some(&f.scene.grid_idx)));
                }
                if !f.scene.solid.is_empty() {
                    self.solid = Some(float_buf(gl, &f.scene.solid, &[3, 3, 3], None));
                }
                self.gen = f.scene.gen;
            }
            let hs = &f.heights;
            if self.hgen != hs.gen && hs.nx > 1 && hs.ny > 1 && hs.h.len() == hs.nx * hs.ny {
                if self.tex_size != (hs.nx, hs.ny) || self.tex_h.is_none() {
                    for t in [self.tex_h.take(), self.tex_p.take()].into_iter().flatten() {
                        gl.delete_texture(t);
                    }
                    self.tex_h = Some(float_tex(gl, hs.nx, hs.ny, &hs.h));
                    let part: Vec<f32> = hs.part.iter().map(|v| if v.is_finite() { *v } else { -1e30 }).collect();
                    self.tex_p = Some(float_tex(gl, hs.nx, hs.ny, &part));
                    self.tex_size = (hs.nx, hs.ny);
                } else {
                    gl.bind_texture(glow::TEXTURE_2D, self.tex_h);
                    gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, hs.nx as i32, hs.ny as i32, glow::RED, glow::FLOAT, glow::PixelUnpackData::Slice(Some(bytes(&hs.h))));
                }
                self.hgen = hs.gen;
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
            // table
            if let Some(b) = &self.solid {
                set(self.lit);
                gl.uniform_1_f32(gl.get_uniform_location(self.lit, "u_alpha").as_ref(), 1.0);
                gl.uniform_1_f32(gl.get_uniform_location(self.lit, "u_shine").as_ref(), 0.15);
                gl.bind_vertex_array(Some(b.vao));
                let split = (f.scene.part_from as i32).min(b.count);
                if split > 0 {
                    gl.draw_arrays(glow::TRIANGLES, 0, split);
                }
            }
            // stock
            if let (Some(b), Some(th), Some(tp)) = (&self.grid, self.tex_h, self.tex_p) {
                set(self.stock);
                let u = |n: &str| gl.get_uniform_location(self.stock, n);
                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D, Some(th));
                gl.uniform_1_i32(u("u_h").as_ref(), 0);
                gl.active_texture(glow::TEXTURE1);
                gl.bind_texture(glow::TEXTURE_2D, Some(tp));
                gl.uniform_1_i32(u("u_part").as_ref(), 1);
                gl.active_texture(glow::TEXTURE0);
                gl.uniform_2_f32(u("u_origin").as_ref(), f.origin[0], f.origin[1]);
                gl.uniform_2_f32(u("u_size").as_ref(), f.size[0], f.size[1]);
                gl.uniform_2_f32(u("u_texel").as_ref(), 1.0 / hs.nx.max(1) as f32, 1.0 / hs.ny.max(1) as f32);
                gl.uniform_1_f32(u("u_floor").as_ref(), f.floor);
                gl.uniform_1_f32(u("u_top").as_ref(), f.top);
                gl.uniform_1_i32(u("u_look").as_ref(), f.look);
                gl.uniform_3_f32(u("u_color").as_ref(), f.color[0], f.color[1], f.color[2]);
                gl.uniform_1_i32(u("u_dev").as_ref(), f.deviation as i32);
                gl.bind_vertex_array(Some(b.vao));
                gl.draw_elements(glow::TRIANGLES, b.count, glow::UNSIGNED_INT, 0);
            }
            // tool (opaque)
            if !f.dynamic.is_empty() {
                set(self.lit);
                gl.uniform_1_f32(gl.get_uniform_location(self.lit, "u_alpha").as_ref(), 1.0);
                gl.uniform_1_f32(gl.get_uniform_location(self.lit, "u_shine").as_ref(), 0.55);
                let n = dynamic(gl, &mut self.scratch_lit, &f.dynamic, &[3, 3, 3]);
                gl.draw_arrays(glow::TRIANGLES, 0, n);
            }
            // paths
            if !f.lines.is_empty() {
                set(self.flat);
                let n = dynamic(gl, &mut self.scratch, &f.lines, &[3, 4]);
                gl.draw_arrays(glow::LINES, 0, n);
            }
            // part ghost last, see-through
            if let Some(b) = &self.solid {
                let split = (f.scene.part_from as i32).min(b.count);
                if f.part_alpha > 0.0 && b.count > split {
                    set(self.lit);
                    gl.uniform_1_f32(gl.get_uniform_location(self.lit, "u_alpha").as_ref(), f.part_alpha);
                    gl.uniform_1_f32(gl.get_uniform_location(self.lit, "u_shine").as_ref(), 0.1);
                    gl.depth_mask(false);
                    gl.bind_vertex_array(Some(b.vao));
                    gl.draw_arrays(glow::TRIANGLES, split, b.count - split);
                    gl.depth_mask(true);
                }
            }
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::DEPTH_TEST);
        }
    }
}
