shader_type spatial;
// Asphalt (R45) with solid edge lines and a dashed centre line. UV.x runs 0–1 across the road,
// UV.y is the distance along the route in metres. The surface is a CC0 asphalt texture
// (ambientCG, docs/CREDITS.md) tinted to `asphalt_color`, smoother and darker in the wheel
// tracks; markings are slightly worn and anti-aliased so they do not flicker in the distance.

uniform sampler2D asphalt_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D asphalt_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform vec3 asphalt_color : source_color = vec3(0.16, 0.16, 0.17);
uniform vec3 marking_color : source_color = vec3(0.92, 0.92, 0.88);
uniform float road_width = 6.0;
uniform float line_width = 0.12;
uniform float dash_length = 3.0;
uniform float dash_gap = 6.0;
// Metres per repeat of the asphalt texture.
uniform float tile = 2.5;
// 0 dry, 1 soaked: darker and glossier asphalt.
uniform float wetness : hint_range(0.0, 1.0) = 0.0;
// 0 plain asphalt colour (Low), 1 textured.
uniform int detail = 1;

float hash(vec2 p) {
	return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

float value_noise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);
	vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(
		mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
		mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x),
		u.y
	);
}

// 1 inside [from, to] across the road, with edges one pixel soft.
float band(float across, float from, float to) {
	float soft = max(fwidth(across), 1e-4);
	return smoothstep(from - soft, from + soft, across) * (1.0 - smoothstep(to - soft, to + soft, across));
}

void fragment() {
	float across = UV.x * road_width;
	float along = UV.y;
	float edges = band(across, 0.25, 0.25 + line_width)
		+ band(across, road_width - 0.25 - line_width, road_width - 0.25);
	float half_line = line_width * 0.5;
	float centre_line = band(across, road_width * 0.5 - half_line, road_width * 0.5 + half_line);
	float dash = mod(along, dash_length + dash_gap);
	float soft = max(fwidth(along), 1e-4);
	centre_line *= 1.0 - smoothstep(dash_length - soft, dash_length + soft, dash);
	// Paint wears off in patches.
	float wear = smoothstep(0.25, 0.6, value_noise(vec2(across * 3.0, along * 0.7)));
	float marking = clamp(edges + centre_line, 0.0, 1.0) * mix(0.55, 1.0, wear);

	// Wheel tracks: a band in each lane, polished darker by traffic.
	float lane = road_width * 0.5;
	float tracks = band(across, lane * 0.3, lane * 0.75) + band(across, lane * 1.25, lane * 1.7);
	vec2 surface = vec2(across, along) / tile;
	vec3 asphalt = asphalt_color;
	vec3 normal_map = vec3(0.5, 0.5, 1.0);
	if (detail > 0) {
		vec3 texture_color = texture(asphalt_albedo, surface).rgb;
		vec3 average = textureLod(asphalt_albedo, vec2(0.5), 12.0).rgb;
		asphalt = asphalt_color * texture_color / max(average, vec3(0.02));
		normal_map = texture(asphalt_normal, surface).rgb;
	}
	asphalt *= 1.0 - tracks * 0.12;
	vec3 albedo = mix(asphalt, marking_color, marking);
	ALBEDO = albedo * (1.0 - wetness * 0.35);
	NORMAL_MAP = normal_map;
	NORMAL_MAP_DEPTH = mix(0.7, 0.3, max(marking, wetness));
	ROUGHNESS = mix(mix(0.88, 0.76, tracks), 0.6, marking);
	ROUGHNESS = mix(ROUGHNESS, 0.15, wetness);
	SPECULAR = mix(0.5, 0.8, wetness);
}
