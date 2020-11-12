#version 140

in vec2 v_tex_coords;
out vec4 color;

uniform sampler2D tex;

void main() {
    float c = texture(tex, v_tex_coords).x;
    color = vec4(c,c,c,1.0);
}
