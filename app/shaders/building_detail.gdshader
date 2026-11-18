shader_type spatial;
#include "res://shaders/winter.gdshaderinc"
#include "res://shaders/building_interior.gdshaderinc"

varying vec3 world_position;
varying vec3 world_normal;

void vertex() {
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	world_normal = normalize(MODEL_NORMAL_MATRIX * NORMAL);
}

vec3 to_linear(vec3 srgb) {
	return mix(srgb / 12.92, pow((srgb + 0.055) / 1.055, vec3(2.4)), step(0.04045, srgb));
}

void fragment() {
	// These fresh, unstored meshes use alpha for trim (0), glass (1/2) and metal (1).
	int style = int(round(COLOR.a * 2.0));
	vec3 albedo = to_linear(COLOR.rgb);
	float roughness = 0.7;
	float metallic = 0.0;
	if (style == 1) {
		float seed = fract(sin(floor(UV.x) * 12.9898) * 43758.5453);
		// Shop panes have V lifted by one (`building_details.rs`).
		vec2 pane = vec2(fract(UV.x), fract(UV.y));
		albedo = UV.y >= 1.0 ? shop_interior(pane, seed) : building_interior(pane, seed);
		roughness = 0.13;
	} else if (style == 2) {
		metallic = 0.7;
		roughness = 0.45;
	}
	float snow = winter_cover(world_position, world_normal);
	ALBEDO = mix(albedo, winter_snow_color.rgb, snow);
	ROUGHNESS = mix(roughness, 0.8, snow);
	METALLIC = metallic * (1.0 - snow);
	SPECULAR = 0.5;
}
