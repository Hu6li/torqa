// Blossom varies per tree, leaving the other crowns in fresh green.
uniform vec4 spring_colors[4] : source_color;
uniform float spring_strength : hint_range(0.0, 1.0) = 0.0;
uniform float spring_from = -10000.0;
uniform sampler2D spring_blossom_texture : source_color, filter_linear_mipmap_anisotropic, repeat_disable;

float spring_amount(vec2 place, float height) {
	float choice = fract(sin(dot(place, vec2(12.9898, 78.233))) * 43758.5453);
	return spring_strength * (1.0 - step(0.32, choice))
		* smoothstep(spring_from, spring_from + 80.0, height);
}

vec3 spring_colour(vec2 place) {
	float choice = fract(sin(dot(place, vec2(37.179, 21.939))) * 43758.5453);
	return spring_colors[int(choice * 3.999)].rgb;
}
