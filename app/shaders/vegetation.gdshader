shader_type spatial;
// The wind clock: the world advances it, and holds it while the ride is paused.
global uniform float wind_time;
// Trees, bushes and rocks from Blender (art/vegetation, R89, ADR 0020), by material:
// 0 bark — the bark texture by the trunk's coordinates (metres around and up);
// 2 fronds — solid palm, banana and fern leaves, tinted per plant (COLOR, sRGB, chosen by the world
//   from the palette), lit through from behind;
// 3 rock — the rock texture laid on from three sides, tinted per rock.
// Foliage cards have their own shader (foliage.gdshader). Trees sway in the wind, more towards
// their tops.

#include "res://shaders/ground.gdshaderinc"
#include "res://shaders/rock.gdshaderinc"
#include "res://shaders/winter.gdshaderinc"

uniform int kind = 0;
uniform sampler2D albedo_texture : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D normal_texture : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform float tile = 1.0;
// How strongly the per-plant colour tints the texture (foliage takes its colour from it).
uniform float tint_strength = 1.0;
// How far a point 10 m up moves in the wind, in metres; trunk and leaves share it.
uniform float sway = 0.0;
// Set by the weather.
uniform float wind_strength : hint_range(0.0, 1.0) = 0.1;

varying vec3 tint;
varying vec3 model_position;
varying vec3 model_normal;
varying vec3 winter_position;
varying vec3 winter_normal;

vec3 to_linear(vec3 srgb) {
	return mix(srgb / 12.92, pow((srgb + 0.055) / 1.055, vec3(2.4)), step(0.04045, srgb));
}

void vertex() {
	model_position = VERTEX;
	model_normal = NORMAL;
	winter_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	winter_normal = normalize(MODEL_NORMAL_MATRIX * NORMAL);
	if (kind == 3 && winter_strength > 0.0) {
		float depth = winter_depth(winter_position) * winter_cover(winter_position, winter_normal);
		VERTEX += (inverse(MODEL_MATRIX) * vec4(0.0, depth, 0.0, 0.0)).xyz;
	}
	// Each plant sways out of step with its neighbours: the phase follows where it stands.
	vec2 place = MODEL_MATRIX[3].xz;
	float phase = place.x * 0.13 + place.y * 0.07;
	float bend = max(VERTEX.y, 0.0) / 10.0;
	vec2 push = vec2(sin(wind_time * 1.3 + phase), cos(wind_time * 1.1 + phase * 1.7)) * bend * bend;
	VERTEX.xz += push * sway * wind_strength * 10.0;
	vec3 plant_colour = to_linear(COLOR.rgb);
	tint = mix(vec3(1.0), plant_colour, tint_strength);
}

void fragment() {
	vec3 albedo;
	float roughness = 0.9;
	if (kind == 3) {
		// Rocks have no texture coordinates: the texture from three sides of the model.
		vec3 blend = pow(abs(model_normal), vec3(4.0));
		blend /= blend.x + blend.y + blend.z;
		vec3 p = model_position / tile;
		albedo = texture(albedo_texture, p.zy).rgb * blend.x
			+ texture(albedo_texture, p.xz).rgb * blend.y
			+ texture(albedo_texture, p.xy).rgb * blend.z;
		vec3 n = model_normal;
		vec3 bent = ground_bend(n, ground_unpack(texture(normal_texture, p.zy)), vec3(0.0, 0.0, 1.0), vec3(0.0, 1.0, 0.0), 1.0) * blend.x
			+ ground_bend(n, ground_unpack(texture(normal_texture, p.xz)), vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0), 1.0) * blend.y
			+ ground_bend(n, ground_unpack(texture(normal_texture, p.xy)), vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), 1.0) * blend.z;
		bent = normalize(bent);
		rock_weathering(model_position, n, 0.65, albedo, bent);
		NORMAL = normalize((VIEW_MATRIX * vec4(MODEL_NORMAL_MATRIX * bent, 0.0)).xyz);
		albedo *= tint;
	} else if (kind == 2) {
		albedo = tint;
		BACKLIGHT = albedo * 0.5;
		roughness = 0.65;
	} else {
		vec2 uv = UV / tile;
		albedo = texture(albedo_texture, uv).rgb;
		NORMAL_MAP = vec3(texture(normal_texture, uv).rg, 1.0);
		NORMAL_MAP_DEPTH = 1.0;
	}
	float snow = winter_cover(winter_position, winter_normal);
	ALBEDO = mix(albedo, winter_snow_color.rgb, snow);
	ROUGHNESS = roughness;
	SPECULAR = 0.3;
}
