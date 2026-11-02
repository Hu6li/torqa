// Shared with terrain: elevation, broken edges, slope and submerged beds limit accumulation.
uniform float winter_strength : hint_range(0.0, 1.0) = 0.0;
uniform float winter_snow_line = 2400.0;
uniform vec4 winter_snow_color : source_color;
uniform vec4 winter_leaf_color : source_color;
uniform float winter_deciduous_from = -10000.0;

float winter_depth(vec3 position) {
	float above = position.y - winter_snow_line;
	return winter_strength * min(0.70, 0.10 + max(above, 0.0) * 0.0002)
		* smoothstep(-60.0, 60.0, above);
}

float winter_load(vec3 position) {
	return clamp((winter_depth(position) - 0.10) / 0.45, 0.0, 1.0);
}

float winter_ground_cover(vec3 position, vec3 normal) {
	if (winter_strength == 0.0) {
		return 0.0;
	}
	float load = winter_load(position);
	float patch = sin(position.x * 0.17 + sin(position.z * 0.11) * 2.0) * 25.0;
	return winter_strength * smoothstep(winter_snow_line - 60.0, winter_snow_line + 60.0, position.y + patch)
		* smoothstep(mix(0.6, 0.42, load), mix(0.8, 0.66, load), normal.y);
}

float winter_cover(vec3 position, vec3 normal) {
	if (winter_strength == 0.0) {
		return 0.0;
	}
	float patch = sin(position.x * 0.17 + sin(position.z * 0.11) * 2.0) * 25.0;
	float load = winter_load(position);
	return winter_strength * smoothstep(winter_snow_line - 60.0, winter_snow_line + 60.0, position.y + patch)
		* smoothstep(mix(0.1, 0.04, load), mix(0.8, 0.6, load), normal.y);
}
