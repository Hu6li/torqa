shader_type spatial;
#include "res://shaders/winter.gdshaderinc"
#include "res://shaders/building_interior.gdshaderinc"
// Materials of the Blender-made building models (R89, ADR 0020, art/buildings): textures baked
// in Blender (plaster, clay tiles, slate, boards, masonry, gravel) by the models' coordinates in
// metres, tinted by palette colours; glass dark and reflective, metal and copper with a sheen,
// sheet roofs with standing seams. Walls darken towards their foot, where rain splashes.
// `pattern` picks how a material behaves, `color` is its palette colour. Buildings vary per
// instance: the instance colour (COLOR) is the plaster, INSTANCE_CUSTOM.rgb the roof colour (on
// flat-roofed buildings the accent colour of their trim), and INSTANCE_CUSTOM.a picks the
// shutters' colour and whether a building has shutters and flowers at all.

const int PLASTER = 0;
const int STONE = 1;
const int WOOD = 2;
const int WOOD_DARK = 3;
const int FRAME = 4;
const int GLASS = 5;
const int SHUTTER = 6;
const int DOOR = 7;
const int TILES = 8;
const int SLATE = 9;
const int METAL = 10;
const int COPPER = 11;
const int FLOWERS = 12;
const int LEAVES = 13;
const int CLOCK = 14;
const int SHEET = 15;
const int GARAGE = 16;
const int LEADED = 17;
const int ACCENT = 18;
const int FLAT_ROOF = 19;
const int PAINTED_WOOD = 20;
const int GLASS_DETAIL = 21;

uniform int pattern = 0;
uniform bool autumn_flowers_hidden = false;
uniform vec3 color : source_color = vec3(0.8);
uniform vec4 shutter_colors[4] : source_color;
uniform vec4 flower_colors[3] : source_color;
uniform vec3 gold : source_color = vec3(0.957, 0.757, 0.431);
uniform bool textured = false;
uniform bool roughness_mapped = false;
uniform sampler2D roughness_texture : filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D albedo_texture : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D normal_texture : hint_normal, filter_linear_mipmap_anisotropic, repeat_enable;
uniform float tile = 2.0;

varying vec3 roof_tint;
varying float variant;
varying vec3 world_position;
varying float height_up;
varying vec3 winter_normal;
varying float window_seed;

// A number in [0, 1) for the lattice point `p`, exact far from the origin (see ground.gdshaderinc).
float hash(vec2 p) {
	uvec2 v = uvec2(ivec2(p)) * 1664525u + 1013904223u;
	v.x += v.y * 1664525u;
	v.y += v.x * 1664525u;
	v ^= v >> 16u;
	v.x += v.y * 1664525u;
	return float(v.x >> 8u) / 16777216.0;
}

vec3 to_linear(vec3 srgb) {
	return mix(srgb / 12.92, pow((srgb + 0.055) / 1.055, vec3(2.4)), step(0.04045, srgb));
}

void vertex() {
	roof_tint = INSTANCE_CUSTOM.rgb;
	variant = INSTANCE_CUSTOM.a;
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	height_up = VERTEX.y;
	winter_normal = normalize(MODEL_NORMAL_MATRIX * NORMAL);
	window_seed = UV2.x;
}

void fragment() {
	if (pattern == FLOWERS && autumn_flowers_hidden) {
		discard;
	}
	vec3 tint = color;
	float roughness = 0.9;
	float specular = 0.4;
	float metallic = 0.0;

	if (pattern == PLASTER || pattern == PAINTED_WOOD) {
		tint = to_linear(COLOR.rgb);
	} else if (pattern == TILES || pattern == ACCENT) {
		// The building's roof colour; models without one keep the material's.
		tint = roof_tint.r + roof_tint.g + roof_tint.b > 0.0 ? to_linear(roof_tint) : color;
	} else if (pattern == GLASS || pattern == LEADED) {
		// Dark panes mirroring the sky, each a little different, as rooms behind them are.
		tint = color * (0.45 + 0.2 * hash(floor(world_position.xz * 0.8) + floor(world_position.y)));
		roughness = 0.04;
		specular = 0.5;
	} else if (pattern == GLASS_DETAIL) {
		tint = building_interior(vec2(UV.x, 1.0 - UV.y), fract(window_seed + variant * 0.713));
		roughness = 0.13;
		specular = 0.5;
	} else if (pattern == SHUTTER) {
		// Some buildings have none; the others one of the palette's shutter colours.
		if (variant < 0.25) {
			discard;
		}
		tint = shutter_colors[int(variant * 97.0) % 4].rgb;
	} else if (pattern == FLOWERS) {
		// Geraniums: one colour per box.
		if (variant > 0.65) {
			discard;
		}
		tint = flower_colors[int(hash(floor(world_position.xz * 0.5)) * 2.999)].rgb;
		roughness = 0.6;
	} else if (pattern == LEAVES) {
		if (variant > 0.65) {
			discard;
		}
		roughness = 0.6;
	} else if (pattern == METAL || pattern == COPPER) {
		metallic = 0.7;
		roughness = 0.45;
	} else if (pattern == SHEET) {
		// Standing seams every half metre along the roof.
		float seam = 1.0 - smoothstep(0.0, 0.03, abs(fract(UV.x * 2.0) - 0.5) - 0.47);
		tint = (roof_tint.r + roof_tint.g + roof_tint.b > 0.0 ? to_linear(roof_tint) : color)
			* (1.0 - 0.25 * seam);
		metallic = 0.5;
		roughness = 0.5;
	} else if (pattern == FRAME) {
		roughness = 0.5;
	} else if (pattern == CLOCK) {
		// A plain dial with a gold ring and hands at ten past ten.
		vec2 centred = (UV - 0.5) * 2.0;
		float ring = step(0.84, length(centred));
		float hands = 0.0;
		for (int i = 0; i < 2; i++) {
			float turn = i == 0 ? 0.847 : 0.167;
			float reach = i == 0 ? 0.5 : 0.72;
			vec2 direction = vec2(sin(turn * 6.2832), cos(turn * 6.2832));
			float along = clamp(dot(centred, direction), 0.0, reach);
			hands = max(hands, step(length(centred - direction * along), 0.07));
		}
		tint = mix(color, gold, max(ring, hands));
	}

	vec3 albedo = tint;
	if (pattern == FLOWERS && winter_strength > 0.5 && world_position.y > winter_deciduous_from) {
		discard;
	}
	if (textured) {
		vec2 uv = UV / tile;
		vec3 texel = texture(albedo_texture, uv).rgb;
		// Plaster, tiles and boards carry light, neutral textures that the tint colours; masonry
		// and gravel keep more of their own.
		albedo = (pattern == STONE || pattern == FLAT_ROOF)
			? texel * mix(vec3(1.0), tint * 1.4, 0.5)
			: texel * tint * 1.25;
		NORMAL_MAP = vec3(texture(normal_texture, uv).rg, 1.0);
		NORMAL_MAP_DEPTH = 1.0;
		if (roughness_mapped) {
			roughness = texture(roughness_texture, uv).r;
		}
	}
	// Walls darken towards the ground, where rain splashes and dirt collects.
	if (pattern == PLASTER || pattern == STONE || pattern == WOOD || pattern == WOOD_DARK
		|| pattern == PAINTED_WOOD) {
		albedo *= mix(0.72, 1.0, smoothstep(-0.2, 1.4, height_up));
	}

	float snow = winter_cover(world_position, winter_normal);
	albedo = mix(albedo, winter_snow_color.rgb, snow);
	roughness = mix(roughness, 0.8, snow);
	metallic *= 1.0 - snow;
	ALBEDO = albedo;
	ROUGHNESS = roughness;
	SPECULAR = specular;
	METALLIC = metallic;
}
