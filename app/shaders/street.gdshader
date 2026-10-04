// Streets, tracks and paths of the map around the route (R45): asphalt for paved streets,
// gravel for tracks and paths (with grass down the middle of tracks), edges darkening into the
// verge. UV.x runs 0–1 across, UV.y is the distance along in metres. CC0 textures from
// ambientCG (docs/CREDITS.md).
shader_type spatial;

uniform sampler2D surface_albedo : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D surface_normal : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform vec3 tint : source_color = vec3(0.17, 0.17, 0.18);
uniform vec3 verge_color : source_color = vec3(0.24, 0.3, 0.15);
// 1 for tracks: grass in the middle where wheels do not run.
uniform float middle_grass : hint_range(0.0, 1.0) = 0.0;
uniform float roughness = 0.88;
// 0 plain colour (Low), 1 textured.
uniform int detail = 1;
uniform float wetness : hint_range(0.0, 1.0) = 0.0;

varying vec3 world_position;

void vertex() {
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}

void fragment() {
	vec2 surface = world_position.xz / 2.5;
	vec3 color = tint;
	vec3 normal_map = vec3(0.5, 0.5, 1.0);
	if (detail > 0) {
		vec3 sample = texture(surface_albedo, surface).rgb;
		vec3 average = textureLod(surface_albedo, vec2(0.5), 12.0).rgb;
		color = tint * sample / max(average, vec3(0.02));
		normal_map = texture(surface_normal, surface).rgb;
	}
	float edge = 1.0 - smoothstep(0.0, 0.12, min(UV.x, 1.0 - UV.x));
	float middle = middle_grass * (1.0 - smoothstep(0.08, 0.16, abs(UV.x - 0.5)));
	color = mix(color, pow(verge_color, vec3(2.2)), max(edge * 0.6, middle * 0.85));
	ALBEDO = color * (1.0 - wetness * 0.3);
	NORMAL_MAP = normal_map;
	NORMAL_MAP_DEPTH = 0.6;
	ROUGHNESS = mix(roughness, 0.25, wetness);
	SPECULAR = 0.35;
}
