mod matrix;

use std::{collections::HashSet, f32::consts::PI};

use casper_node::consensus::utils::ValidatorMap;
use glium::{
    implement_vertex, index, uniform, Display, DrawParameters, Frame, Program, Surface,
    VertexBuffer,
};
use glium_text_rusttype::{self, FontTexture, TextDisplay, TextSystem};
use nalgebra::Vector2;

use crate::{renderer::matrix::Matrix, Graph, GraphUnit, UnitId};

const VERTEX_SHADER_SRC: &str = r#"
    #version 140

    in vec2 position;

    uniform mat4 matrix;
    uniform vec3 color;
    out vec3 in_color;

    void main() {
        gl_Position = matrix * vec4(position, 0.0, 1.0);
        in_color = color;
    }
"#;

const FRAGMENT_SHADER_SRC: &str = r#"
    #version 140

    in vec3 in_color;
    out vec4 color;

    void main() {
        color = vec4(in_color, 1.0);
    }
"#;

const FONT_FILE: &[u8] = include_bytes!("../DejaVuSans.ttf");

#[derive(Debug, Clone, Copy)]
struct Vertex {
    position: [f32; 2],
}

implement_vertex!(Vertex, position);

/// Rendering-specific data.
pub struct Renderer {
    /// The coordinates at the center of the screen.
    center: Vector2<f32>,
    /// The width of the window, in pixels.
    window_width: f32,
    /// The current width of the viewport.
    width: f32,
    /// The shading program.
    program: Program,
    /// Stuff for rendering text.
    text_system: TextSystem,
    font: FontTexture,

    /// Pre-generated vertices for a unit.
    unit_vertex_buffer: VertexBuffer<Vertex>,
    interior_indices: index::NoIndices,
    frame_indices: index::IndexBuffer<u32>,

    /// `True` if we're drawing edges.
    edges_enabled: bool,
}

const UNIT_WIDTH: f32 = 0.5;
const UNIT_HEIGHT: f32 = 0.4;
const CORNER_RADIUS: f32 = 0.05;
const LINE_WIDTH: f32 = 0.015;

impl Renderer {
    pub fn new(display: &Display) -> Self {
        let text_system = TextSystem::new(display);
        let font =
            FontTexture::new(display, FONT_FILE, 32, FontTexture::ascii_character_list()).unwrap();

        let (unit_vertex_buffer, interior_indices, frame_indices) =
            Self::unit_vertex_buffer(display);

        Renderer {
            center: Vector2::new(3.5, 2.5),
            window_width: 3000.0, // will get updated on first frame draw
            width: 8.0,
            program: Program::from_source(display, VERTEX_SHADER_SRC, FRAGMENT_SHADER_SRC, None)
                .unwrap(),
            text_system,
            font,

            unit_vertex_buffer,
            interior_indices,
            frame_indices,
            edges_enabled: true,
        }
    }

    /// Creates vertices for a rounded rectangle.
    fn unit_vertex_buffer(
        display: &Display,
    ) -> (
        VertexBuffer<Vertex>,
        index::NoIndices,
        index::IndexBuffer<u32>,
    ) {
        let mut shape = vec![];
        let n_vertices_corner = 8;

        let corner_radius = CORNER_RADIUS;
        let width = UNIT_WIDTH;
        let height = UNIT_HEIGHT;

        let corners = [
            (
                width / 2.0 - corner_radius,
                height / 2.0 - corner_radius,
                0.0,
            ),
            (
                -width / 2.0 + corner_radius,
                height / 2.0 - corner_radius,
                PI * 0.5,
            ),
            (
                -width / 2.0 + corner_radius,
                -height / 2.0 + corner_radius,
                PI,
            ),
            (
                width / 2.0 - corner_radius,
                -height / 2.0 + corner_radius,
                PI * 1.5,
            ),
        ];

        shape.push(Vertex {
            position: [0.0, 0.0],
        });
        for (x, y, phase) in corners {
            for i in 0..n_vertices_corner {
                let ang = 0.5 * PI * (i as f32) / n_vertices_corner as f32 + phase;
                shape.push(Vertex {
                    position: [corner_radius * ang.cos() + x, corner_radius * ang.sin() + y],
                });
            }
        }
        shape.push(shape[1]);

        (
            VertexBuffer::new(display, &shape).unwrap(),
            index::NoIndices(index::PrimitiveType::TriangleFan),
            index::IndexBuffer::new(
                display,
                index::PrimitiveType::LineLoop,
                &(1..(shape.len() - 1) as u32).collect::<Vec<_>>(),
            )
            .unwrap(),
        )
    }

    /// Draws the graph.
    pub fn draw(&mut self, display: &Display, graph: &Graph, cursor_x: f32, cursor_y: f32) {
        let mut target = display.draw();

        let (size_x, size_y) = target.get_dimensions();
        self.window_width = size_x as f32;

        let (cursor_x, cursor_y) = self.convert_cursor(cursor_x, cursor_y, size_x, size_y);

        let aspect = (size_y as f32) / (size_x as f32);

        let height = self.width * aspect;

        let max_graph_height = (self.center.y + height / 2.0 + 1.0) as usize;
        let min_graph_height = (self.center.y - height / 2.0 - 1.0).max(0.0) as usize;

        let max_validator_index = (self.center.x + self.width / 2.0 + 1.0) as usize;
        let min_validator_index = (self.center.x - self.width / 2.0 - 1.0).max(0.0) as usize;

        target.clear_color(0.0, 0.0, 0.2, 1.0);

        let matrix = Matrix::translation(-self.center.x, -self.center.y)
            * Matrix::scale(2.0 / self.width, 2.0 / height);

        let mut edges_to_draw = HashSet::new();
        let mut highlighted_edges_to_draw = HashSet::new();

        for unit in graph.iter_range(
            min_validator_index..=max_validator_index,
            min_graph_height..=max_graph_height,
        ) {
            let set_to_insert = if Self::unit_contains_cursor(unit, cursor_x, cursor_y) {
                &mut highlighted_edges_to_draw
            } else {
                &mut edges_to_draw
            };
            for cited_unit in &unit.cited_units {
                set_to_insert.insert((unit.id, *cited_unit));
            }
            for dependent_unit in graph.reverse_edges.get(&unit.id).into_iter().flatten() {
                set_to_insert.insert((*dependent_unit, unit.id));
            }
        }

        // draw edges first, so that the units are drawn over them
        if self.edges_enabled {
            self.draw_edges(display, &mut target, &matrix, graph, edges_to_draw, false);
        }
        self.draw_edges(
            display,
            &mut target,
            &matrix,
            graph,
            highlighted_edges_to_draw,
            true,
        );

        for unit in graph.iter_range(
            min_validator_index..=max_validator_index,
            min_graph_height..=max_graph_height,
        ) {
            self.draw_unit(&mut target, unit, graph.validator_weights(), &matrix);
        }

        target.finish().unwrap();
    }

    /// Converts the cursor coordinates in pixels into the scene coordinates.
    fn convert_cursor(&self, cursor_x: f32, cursor_y: f32, size_x: u32, size_y: u32) -> (f32, f32) {
        let size_x = size_x as f32;
        let size_y = size_y as f32;
        let delta_x = (cursor_x / size_x - 0.5) * self.width;
        let delta_y = (0.5 - cursor_y / size_y) * self.width * size_y / size_x;
        (self.center.x + delta_x, self.center.y + delta_y)
    }

    /// Checks whether the cursor hovers over a unit.
    fn unit_contains_cursor(unit: &GraphUnit, cursor_x: f32, cursor_y: f32) -> bool {
        let (unit_x, unit_y) = Self::unit_pos(unit);
        (unit_x - cursor_x).abs() < UNIT_WIDTH / 2.0
            && (unit_y - cursor_y).abs() < UNIT_HEIGHT / 2.0
    }

    /// Draws a unit.
    fn draw_unit(
        &mut self,
        target: &mut Frame,
        unit: &GraphUnit,
        weights: &ValidatorMap<f32>,
        view: &Matrix,
    ) {
        let (x, y) = Self::unit_pos(unit);

        let matrix2 = Matrix::translation(x, y) * *view;

        let color = match (unit.is_proposal, unit.max_quorum.as_ref()) {
            (false, Some(quorum)) => {
                if quorum.max_rank <= 1 {
                    Self::quorum_color_spectrum(0.0)
                } else {
                    let frac = quorum.rank as f32 / (quorum.max_rank - 1) as f32;
                    Self::quorum_color_spectrum(frac)
                }
            }
            (true, _) => [0.0_f32, 0.5, 0.5],
            _ => [0.0_f32, 0.0, 0.2],
        };

        let uniforms = uniform! {
            matrix: matrix2.inner(),
            color: color,
        };

        target
            .draw(
                &self.unit_vertex_buffer,
                self.interior_indices,
                &self.program,
                &uniforms,
                &Default::default(),
            )
            .unwrap();

        let uniforms = uniform! {
            matrix: matrix2.inner(),
            color: [ 1.0_f32, 1.0, 0.0 ],
        };

        let draw_params = DrawParameters {
            line_width: Some(LINE_WIDTH),
            ..Default::default()
        };

        target
            .draw(
                &self.unit_vertex_buffer,
                &self.frame_indices,
                &self.program,
                &uniforms,
                &draw_params,
            )
            .unwrap();

        if self.width < 10.0 {
            let text1 = format!("{:?}", unit.id);
            let text2 = format!(
                "Creator weight: {:3.1}%",
                weights.get(unit.creator).unwrap()
            );
            let text3 = format!("Vote: {:?}", unit.vote);
            let text4 = format!("round_exp: {}", unit.round_exp);
            let text5 = format!("round_id: {}", unit.round_id);
            let text6 = format!("timestamp: {} (round {})", unit.timestamp, unit.round_num);
            let text7 = if let Some(quorum) = unit.max_quorum.as_ref() {
                format!("max quorum: {:3.1}%", quorum.weight_percent)
            } else {
                "".to_string()
            };
            self.draw_text(target, -0.4, 0.7, &text1, 1.3, &matrix2);
            self.draw_text(target, -0.8, 0.46, &text2, 0.8, &matrix2);
            self.draw_text(target, -0.8, 0.22, &text3, 0.8, &matrix2);
            self.draw_text(target, -0.8, -0.02, &text4, 0.8, &matrix2);
            self.draw_text(target, -0.8, -0.26, &text5, 0.8, &matrix2);
            self.draw_text(target, -0.8, -0.5, &text6, 0.8, &matrix2);
            self.draw_text(target, -0.8, -0.74, &text7, 0.8, &matrix2);
        } else {
            let text = format!("{:?}", unit.id);
            self.draw_text(target, -0.4, -0.15, &text, 3.0, &matrix2);
        }
    }

    /// Renders a string.
    fn draw_text(
        &self,
        target: &mut Frame,
        x: f32,
        y: f32,
        text: &str,
        scale: f32,
        matrix: &Matrix,
    ) {
        let basic_scale = UNIT_HEIGHT / 12.0;
        let scale = basic_scale * scale;
        let matrix = Matrix::scale(scale, scale)
            * Matrix::translation(x * UNIT_WIDTH / 2.0, y * UNIT_HEIGHT / 2.0)
            * *matrix;
        let text = TextDisplay::new(&self.text_system, &self.font, text);

        glium_text_rusttype::draw(
            &text,
            &self.text_system,
            target,
            matrix.inner(),
            (1.0, 1.0, 1.0, 1.0),
        )
        .unwrap();
    }

    /// Draws the edges between units.
    fn draw_edges(
        &mut self,
        display: &Display,
        target: &mut Frame,
        view: &Matrix,
        graph: &Graph,
        edges: HashSet<(UnitId, UnitId)>,
        highlight: bool,
    ) {
        let mut vertices = vec![];

        for (unit1, unit2) in edges {
            let pos1 = Self::unit_pos(graph.get(&unit1).unwrap());
            let pos2 = Self::unit_pos(graph.get(&unit2).unwrap());

            vertices.push(Vertex {
                position: [pos1.0, pos1.1],
            });
            vertices.push(Vertex {
                position: [pos2.0, pos2.1],
            });
        }

        let vertex_buffer = VertexBuffer::new(display, &vertices).unwrap();
        let indices = index::NoIndices(index::PrimitiveType::LinesList);

        let color = if highlight {
            [1.0_f32, 1.0, 1.0]
        } else {
            [1.0_f32, 1.0, 0.0]
        };

        let uniforms = uniform! {
            matrix: view.inner(),
            color: color,
        };

        let draw_parameters = DrawParameters {
            line_width: Some(if highlight {
                LINE_WIDTH * 2.0
            } else {
                LINE_WIDTH
            }),
            ..Default::default()
        };

        target
            .draw(
                &vertex_buffer,
                indices,
                &self.program,
                &uniforms,
                &draw_parameters,
            )
            .unwrap();
    }

    /// Returns the position of the units in scene coordinates.
    fn unit_pos(unit: &GraphUnit) -> (f32, f32) {
        let x = unit.creator.0 as f32;
        let y = unit.graph_height as f32;
        (x, y)
    }

    /// Handles a mouse scroll event (zooms in or out).
    pub fn mouse_scroll(&mut self, lines: f32) {
        self.width *= 2.0_f32.powf(lines / 3.0);
    }

    /// Handles a dragging event (pans the screen).
    pub fn pan(&mut self, delta_x: f32, delta_y: f32) {
        let scale = self.width / self.window_width;
        self.center += Vector2::new(-delta_x * scale, delta_y * scale);
    }

    pub fn toggle_edges(&mut self) {
        self.edges_enabled = !self.edges_enabled;
    }

    /// Returns a color for the max quorum based on its rank.
    fn quorum_color_spectrum(frac: f32) -> [f32; 3] {
        let r = if frac < 0.5 { frac } else { 1.0 };
        let g = if frac < 0.5 { 1.0 } else { 1.0 - frac };
        [r * 0.5, g * 0.5, 0.0]
    }
}
