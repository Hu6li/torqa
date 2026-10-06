shader_type spatial;
// Materials of the Blender-made building models (R45, ADR 0011, art/buildings): flat palette
// colours, faceted. `pattern` picks how a material behaves, `color` is its palette colour.
// Buildings vary per instance: the instance colour (COLOR) is the plaster, INSTANCE_CUSTOM.rgb
// the roof colour (on flat-roofed buildings the accent colour of their trim), and
// INSTANCE_CUSTOM.a picks the shutters' colour and whether a building has shutters and flowers
// at all.

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

uniform int pattern = 0;
uniform vec3 color : source_color = vec3(0.8);
uniform vec4 shutter_colors[4] : source_color;
uniform vec4 flower_colors[3] : source_color;
uniform vec3 gold : source_color = vec3(0.957, 0.757, 0.431);

varying vec3 roof_tint;
varying float variant;
varying vec3 world_position;

float hash(vec2 p) {
	return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

vec3 to_linear(vec3 srgb) {
	return mix(srgb / 12.92, pow((srgb + 0.055) / 1.055, vec3(2.4)), step(0.04045, srgb));
}

void vertex() {
	roof_tint = INSTANCE_CUSTOM.rgb;
	variant = INSTANCE_CUSTOM.a;
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}

void fragment() {
	NORMAL = normalize(cross(dFdy(VERTEX), dFdx(VERTEX)));
	vec3 albedo = color;
	float roughness = 0.9;
	float specular = 0.2;

	if (pattern == PLASTER) {
		albedo = to_linear(COLOR.rgb);
	} else if (pattern == TILES || pattern == ACCENT) {
		// The building's roof colour; models without one keep the material's.
		albedo = roof_tint.r + roof_tint.g + roof_tint.b > 0.0 ? to_linear(roof_tint) : color;
	} else if (pattern == GLASS || pattern == LEADED) {
		// A soft sheen, nothing mirror-like.
		roughness = 0.35;
		specular = 0.4;
	} else if (pattern == SHUTTER) {
		// Some buildings have none; the others one of the palette's shutter colours.
		if (variant < 0.25) {
			discard;
		}
		albedo = shutter_colors[int(variant * 97.0) % 4].rgb;
	} else if (pattern == FLOWERS) {
		// Geraniums: one colour per box.
		if (variant > 0.65) {
			discard;
		}
		albedo = flower_colors[int(hash(floor(world_position.xz * 0.5)) * 2.999)].rgb;
	} else if (pattern == LEAVES) {
		if (variant > 0.65) {
			discard;
		}
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
		albedo = mix(color, gold, max(ring, hands));
	}

	ALBEDO = albedo;
	ROUGHNESS = roughness;
	SPECULAR = specular;
}
