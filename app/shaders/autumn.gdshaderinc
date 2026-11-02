// Broadleaves change colour; evergreen lowlands and needles keep their own palette.
uniform vec4 autumn_colors[4] : source_color;
uniform float autumn_strength : hint_range(0.0, 1.0) = 0.0;
uniform float autumn_from = -10000.0;

float autumn_amount(float height) {
	return autumn_strength * smoothstep(autumn_from, autumn_from + 80.0, height);
}

float autumn_hash(vec2 place) {
	uvec2 value = uvec2(ivec2(place)) * 1664525u + 1013904223u;
	value.x += value.y * 1664525u;
	value.y += value.x * 1664525u;
	value ^= value >> 16u;
	value.x += value.y * 1664525u;
	return float(value.x >> 8u) / 16777216.0;
}

float autumn_noise(vec2 place) {
	vec2 cell = floor(place);
	vec2 part = fract(place);
	part = part * part * (3.0 - 2.0 * part);
	return mix(mix(autumn_hash(cell), autumn_hash(cell + vec2(1, 0)), part.x),
		mix(autumn_hash(cell + vec2(0, 1)), autumn_hash(cell + vec2(1, 1)), part.x), part.y);
}

float autumn_collection(vec2 place) {
	return smoothstep(0.18, 0.55, autumn_noise(place / 170.0));
}

vec3 autumn_colour(vec2 place) {
	float choice = autumn_hash(floor(place / 5.0));
	return autumn_colors[int(choice * 3.999)].rgb;
}
