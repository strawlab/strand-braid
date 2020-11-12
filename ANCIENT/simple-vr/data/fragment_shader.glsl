#version 140

in vec2 v_tex_coords;
out vec4 color;

uniform sampler2D tex;

void main() {
    vec3 tex_color = texture(tex, v_tex_coords).rgb;
    color = vec4(tex_color, 1.0);
}
