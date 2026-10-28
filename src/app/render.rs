//! OpenGL (glow) renderer for the 3D viewport: shaded solid, model edges, highlights,
//! translucent datum planes / previews and coloured overlay lines, with section clipping.

use eframe::glow::{self, HasContext};
use glam::Mat4;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style { Reflect, ShadedEdges, Shaded, NoHidden, HiddenLine, Wireframe }

impl Style {
    /// In Creo's order; Strg+1 … Strg+6.
    pub const ALL: [Style; 6] = [Style::Reflect, Style::ShadedEdges, Style::Shaded, Style::NoHidden, Style::HiddenLine, Style::Wireframe];
    pub fn name(&self) -> &'static str {
        match self {
            Style::Reflect => "Schattierung mit Reflexion", Style::ShadedEdges => "Schattierung mit Kanten", Style::Shaded => "Schattierung",
            Style::NoHidden => "Sichtbare Kanten", Style::HiddenLine => "Verdeckte Kanten", Style::Wireframe => "Drahtmodell",
        }
    }
    pub fn shortcut(&self) -> &'static str {
        match self { Style::Reflect => "Strg+1", Style::ShadedEdges => "Strg+2", Style::Shaded => "Strg+3", Style::NoHidden => "Strg+4", Style::HiddenLine => "Strg+5", Style::Wireframe => "Strg+6" }
    }
}

/// Geometry that only changes on regeneration.
pub struct ModelBuffers {
    pub generation: u64,
    /// pos(3) normal(3) colour(3) — the colour multiplies the frame colour (assembly components)
    pub solid: Vec<f32>,
    /// pos(3)
    pub edges: Vec<f32>,
}

/// Per-frame draw request.
pub struct FrameData {
    pub mvp: Mat4,
    pub view: Mat4,
    pub model: Arc<ModelBuffers>,
    pub color: [f32; 3],
    pub opacity: f32,
    pub style: Style,
    pub section: Option<[f32; 4]>,
    /// pos(3) normal(3) — highlighted faces
    pub hl_tris: Vec<f32>,
    pub hl_color: [f32; 4],
    pub hover_tris: Vec<f32>,
    /// pos(3) rgba(4)
    pub lines: Vec<f32>,
    /// pos(3) rgba(4), drawn translucent without depth writes
    pub glass: Vec<f32>,
    pub depth_bias: f32,
}

struct Mesh { vao: glow::VertexArray, vbo: glow::Buffer, count: i32 }

pub struct Renderer {
    lit: glow::Program,
    flat: glow::Program,
    solid: Option<Mesh>,
    edges: Option<Mesh>,
    generation: u64,
    scratch: Option<(glow::VertexArray, glow::Buffer)>,
    scratch_lit: Option<(glow::VertexArray, glow::Buffer)>,
}

const LIT_VS: &str = r#"
in vec3 a_pos; in vec3 a_nrm; in vec3 a_col;
uniform mat4 u_mvp; uniform mat4 u_view;
out vec3 v_nrm; out vec3 v_world; out vec3 v_col;
void main() {
    v_nrm = mat3(u_view) * a_nrm;
    v_col = a_col;
    v_world = a_pos;
    gl_Position = u_mvp * vec4(a_pos, 1.0);
}"#;

const LIT_FS: &str = r#"
in vec3 v_nrm; in vec3 v_world; in vec3 v_col;
uniform vec4 u_color; uniform int u_mode; uniform vec4 u_section; uniform int u_clip;
out vec4 frag;
void main() {
    if (u_clip == 1 && dot(v_world, u_section.xyz) - u_section.w > 0.0) discard;
    vec3 n = normalize(v_nrm);
    if (!gl_FrontFacing) n = -n;
    if (u_mode == 1) { frag = u_color; return; }
    if (u_mode == 2) { frag = vec4(1.0, 1.0, 1.0, 1.0); return; }
    vec3 l1 = normalize(vec3(0.35, 0.55, 1.0));
    vec3 l2 = normalize(vec3(-0.6, -0.2, 0.5));
    float d = max(dot(n, l1), 0.0) * 0.58 + max(dot(n, l2), 0.0) * 0.24;
    vec3 h = normalize(l1 + vec3(0.0, 0.0, 1.0));
    float s = pow(max(dot(n, h), 0.0), 48.0) * 0.35;
    vec3 c = u_color.rgb * v_col * (0.46 + d) + vec3(s);
    if (u_mode == 3) {
        // Schattierung mit Reflexion: sky/floor environment reflected in the surface
        vec3 r = reflect(vec3(0.0, 0.0, -1.0), n);
        vec3 env = mix(vec3(0.30, 0.33, 0.38), vec3(0.97, 0.98, 1.0), smoothstep(-0.25, 0.35, r.y));
        c = mix(c, env * (0.55 + 0.45 * u_color.rgb * v_col), 0.32) + vec3(pow(max(dot(n, h), 0.0), 96.0) * 0.55);
    }
    if (!gl_FrontFacing) c = vec3(0.85, 0.45, 0.35) * (0.4 + d * 0.6);
    frag = vec4(c, u_color.a);
}"#;

const FLAT_VS: &str = r#"
in vec3 a_pos; in vec4 a_col;
uniform mat4 u_mvp; uniform float u_bias;
out vec4 v_col; out vec3 v_world;
void main() {
    v_col = a_col; v_world = a_pos;
    gl_Position = u_mvp * vec4(a_pos, 1.0);
    gl_Position.z -= u_bias * gl_Position.w;
}"#;

const FLAT_FS: &str = r#"
in vec4 v_col; in vec3 v_world;
uniform vec4 u_section; uniform int u_clip;
out vec4 frag;
void main() {
    if (u_clip == 1 && dot(v_world, u_section.xyz) - u_section.w > 0.0) discard;
    frag = v_col;
}"#;

unsafe fn program(gl: &glow::Context, vs: &str, fs: &str, attrs: &[&str]) -> glow::Program {
    let header = "#version 150\n";
    let p = gl.create_program().expect("program");
    let mut shaders = vec![];
    for (ty, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
        let s = gl.create_shader(ty).expect("shader");
        gl.shader_source(s, &format!("{}{}", header, src));
        gl.compile_shader(s);
        if !gl.get_shader_compile_status(s) { panic!("shader compile error: {}", gl.get_shader_info_log(s)); }
        gl.attach_shader(p, s);
        shaders.push(s);
    }
    for (i, a) in attrs.iter().enumerate() { gl.bind_attrib_location(p, i as u32, a); }
    gl.bind_frag_data_location(p, 0, "frag");
    gl.link_program(p);
    if !gl.get_program_link_status(p) { panic!("link error: {}", gl.get_program_info_log(p)); }
    for s in shaders { gl.detach_shader(p, s); gl.delete_shader(s); }
    p
}

unsafe fn upload(gl: &glow::Context, data: &[f32], layout: &[i32]) -> Mesh {
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
    Mesh { vao, vbo, count: (data.len() as i32) / stride }
}

fn bytes(d: &[f32]) -> &[u8] { unsafe { std::slice::from_raw_parts(d.as_ptr() as *const u8, std::mem::size_of_val(d)) } }

impl Renderer {
    pub fn new(gl: &glow::Context) -> Renderer {
        unsafe {
            Renderer {
                lit: program(gl, LIT_VS, LIT_FS, &["a_pos", "a_nrm", "a_col"]),
                flat: program(gl, FLAT_VS, FLAT_FS, &["a_pos", "a_col"]),
                solid: None,
                edges: None,
                generation: 0,
                scratch: None,
                scratch_lit: None,
            }
        }
    }

    unsafe fn free(gl: &glow::Context, m: Option<Mesh>) {
        if let Some(m) = m { gl.delete_vertex_array(m.vao); gl.delete_buffer(m.vbo); }
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

    pub fn paint(&mut self, gl: &glow::Context, f: &FrameData) {
        unsafe {
            if self.generation != f.model.generation {
                Self::free(gl, self.solid.take());
                Self::free(gl, self.edges.take());
                if !f.model.solid.is_empty() { self.solid = Some(upload(gl, &f.model.solid, &[3, 3, 3])); }
                if !f.model.edges.is_empty() {
                    let colored: Vec<f32> = f.model.edges.chunks(3).flat_map(|p| [p[0], p[1], p[2], 0.0, 0.0, 0.0, 1.0]).collect();
                    self.edges = Some(upload(gl, &colored, &[3, 4]));
                }
                self.generation = f.model.generation;
            }
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.depth_mask(true);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);

            let clip = f.section.is_some() as i32;
            let sec = f.section.unwrap_or([0.0; 4]);
            let set_common = |p: glow::Program| {
                gl.use_program(Some(p));
                gl.uniform_matrix_4_f32_slice(gl.get_uniform_location(p, "u_mvp").as_ref(), false, &f.mvp.to_cols_array());
                gl.uniform_4_f32(gl.get_uniform_location(p, "u_section").as_ref(), sec[0], sec[1], sec[2], sec[3]);
                gl.uniform_1_i32(gl.get_uniform_location(p, "u_clip").as_ref(), clip);
            };

            // solid
            let draw_solid = !matches!(f.style, Style::Wireframe);
            set_common(self.lit);
            gl.uniform_matrix_4_f32_slice(gl.get_uniform_location(self.lit, "u_view").as_ref(), false, &f.view.to_cols_array());
            let mode = match f.style { Style::HiddenLine | Style::NoHidden => 2, Style::Reflect => 3, _ => 0 };
            if let (Some(m), true) = (&self.solid, draw_solid) {
                gl.uniform_1_i32(gl.get_uniform_location(self.lit, "u_mode").as_ref(), mode);
                gl.uniform_4_f32(gl.get_uniform_location(self.lit, "u_color").as_ref(), f.color[0], f.color[1], f.color[2], if mode != 2 { f.opacity } else { 1.0 });
                if f.opacity < 0.999 && mode != 2 { gl.depth_mask(false); }
                gl.enable(glow::POLYGON_OFFSET_FILL);
                gl.polygon_offset(1.0, 1.0);
                gl.bind_vertex_array(Some(m.vao));
                gl.draw_arrays(glow::TRIANGLES, 0, m.count);
                gl.disable(glow::POLYGON_OFFSET_FILL);
                gl.depth_mask(true);
            }
            // highlights (selected / preselected faces)
            for (tris, col) in [(&f.hover_tris, [0.35, 0.75, 1.0, 0.55]), (&f.hl_tris, f.hl_color)] {
                if tris.is_empty() { continue; }
                gl.use_program(Some(self.lit));
                gl.uniform_1_i32(gl.get_uniform_location(self.lit, "u_mode").as_ref(), 1);
                gl.uniform_4_f32(gl.get_uniform_location(self.lit, "u_color").as_ref(), col[0], col[1], col[2], col[3]);
                let n = Self::dynamic(gl, &mut self.scratch_lit, tris, &[3, 3]);
                gl.depth_mask(false);
                gl.draw_arrays(glow::TRIANGLES, 0, n);
                gl.depth_mask(true);
            }
            // edges
            set_common(self.flat);
            let bias_loc = gl.get_uniform_location(self.flat, "u_bias");
            if let Some(m) = &self.edges {
                gl.bind_vertex_array(Some(m.vao));
                match f.style {
                    Style::Shaded | Style::Reflect => {}
                    Style::HiddenLine => {
                        // hidden edges faint, then visible edges dark
                        gl.disable(glow::DEPTH_TEST);
                        gl.uniform_1_f32(bias_loc.as_ref(), 0.0);
                        gl.blend_color(0.0, 0.0, 0.0, 0.25);
                        gl.blend_func(glow::CONSTANT_ALPHA, glow::ONE_MINUS_CONSTANT_ALPHA);
                        gl.draw_arrays(glow::LINES, 0, m.count);
                        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                        gl.enable(glow::DEPTH_TEST);
                        gl.uniform_1_f32(bias_loc.as_ref(), f.depth_bias);
                        gl.draw_arrays(glow::LINES, 0, m.count);
                    }
                    Style::Wireframe => {
                        gl.disable(glow::DEPTH_TEST);
                        gl.draw_arrays(glow::LINES, 0, m.count);
                        gl.enable(glow::DEPTH_TEST);
                    }
                    _ => {
                        gl.uniform_1_f32(bias_loc.as_ref(), f.depth_bias);
                        gl.draw_arrays(glow::LINES, 0, m.count);
                    }
                }
            }
            // overlay lines (datums, curves, axes)
            gl.uniform_1_f32(bias_loc.as_ref(), f.depth_bias * 2.0);
            if !f.lines.is_empty() {
                let n = Self::dynamic(gl, &mut self.scratch, &f.lines, &[3, 4]);
                gl.draw_arrays(glow::LINES, 0, n);
            }
            // translucent planes / previews
            if !f.glass.is_empty() {
                gl.uniform_1_f32(bias_loc.as_ref(), 0.0);
                gl.depth_mask(false);
                let n = Self::dynamic(gl, &mut self.scratch, &f.glass, &[3, 4]);
                gl.draw_arrays(glow::TRIANGLES, 0, n);
                gl.depth_mask(true);
            }
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::DEPTH_TEST);
        }
    }
}
