#version 140

uniform	mat4 projection_matrix;
uniform	mat4 view_matrix;
uniform	mat4 model_matrix;

in vec3 position;
in vec2 tex_coords;
out vec2 v_tex_coords;

void main() {
    v_tex_coords = tex_coords;
    gl_Position = projection_matrix * view_matrix * model_matrix * vec4(position, 1.0);
}
