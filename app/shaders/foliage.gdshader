shader_type spatial;
render_mode cull_disabled;
// The wind clock: the world advances it, and holds it while the ride is paused.
global uniform float wind_time;
#include "res://shaders/winter.gdshaderinc"
uniform bool winter_broadleaf = false;
#include "res://shaders/spring.gdshaderinc"
#include "res://shaders/autumn.gdshaderinc"
// Foliage cards of trees and bushes (art/vegetation, R89, ADR 0020): clusters of leaves or
// sprays of needles cut out by their alpha, seen from both sides, tinted per plant (COLOR, sRGB,
// chosen by the world from the palette), lit through from behind when the sun is beyond them,
// swaying with the tree and fluttering on their own.

uniform sampler2D albedo_texture : source_color, filter_linear_mipmap_anisotropic, repeat_disable;
// How far a point 10 m up moves in the wind, in metres; the trunk's material sways the same.
uniform float sway = 0.0;
// Set by the weather.
uniform float wind_strength : hint_range(0.0, 1.0) = 0.1;

varying vec3 tint;
varying vec3 winter_position;
varying vec3 winter_normal;
varying float winter_dormancy;
varying float flowering;

vec3 to_linear(vec3 srgb) {
	return mix(srgb / 12.92, pow((srgb + 0.055) / 1.055, vec3(2.4)), step(0.04045, srgb));
}

void vertex() {
	// As vegetation.gdshader, so cards stay on their branches.
	vec2 place = MODEL_MATRIX[3].xz;
	float phase = place.x * 0.13 + place.y * 0.07;
	float bend = max(VERTEX.y, 0.0) / 10.0;
	vec2 push = vec2(sin(wind_time * 1.3 + phase), cos(wind_time * 1.1 + phase * 1.7)) * bend * bend;
	VERTEX.xz += push * sway * wind_strength * 10.0;
	// Each card flutters out of step with its neighbours.
	float flutter = sin(wind_time * 6.0 + dot(VERTEX, vec3(3.1, 1.7, 2.3)));
	VERTEX += NORMAL * flutter * 0.2 * wind_strength;
	tint = to_linear(COLOR.rgb);
	float height = (MODEL_MATRIX * vec4(VERTEX, 1.0)).y;
	tint = mix(tint, autumn_colour(place), autumn_amount(height));
	flowering = spring_amount(place, height);
	tint = mix(tint, spring_colour(place), flowering);
	winter_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	winter_normal = normalize(MODEL_NORMAL_MATRIX * NORMAL);
	winter_dormancy = winter_broadleaf ? winter_strength
		* smoothstep(winter_deciduous_from, winter_deciduous_from + 80.0, winter_position.y) : 0.0;
	tint = mix(tint, winter_leaf_color.rgb, winter_dormancy);
}

void fragment() {
	vec4 texel = texture(albedo_texture, UV);
	if (flowering > 0.0) {
		texel = mix(texel, texture(spring_blossom_texture, UV), flowering);
	}
	ALBEDO = texel.rgb * tint;
	float snow = winter_cover(winter_position, winter_normal);
	ALBEDO = mix(ALBEDO, winter_snow_color.rgb, snow * 0.9);
	ALPHA = texel.a;
	ALPHA_SCISSOR_THRESHOLD = mix(0.5, 0.93, winter_dormancy);
	// Godot turns a back face's normal round; a card's normal points out of the crown on both
	// sides, so the crown is lit as one volume.
	if (!FRONT_FACING) {
		NORMAL = -NORMAL;
	}
	BACKLIGHT = ALBEDO * mix(0.6, 0.15, snow);
	ROUGHNESS = 0.7;
	SPECULAR = 0.3;
}
