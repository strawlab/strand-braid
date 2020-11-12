#version 140

in vec2 v_tex_coords;
out vec4 color;

uniform sampler2D tex;

void main() {
    vec3 c = texture(tex, v_tex_coords).rgb;
    color = vec4(c.r,c.g,c.b,1.0);
}
