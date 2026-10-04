shader_type spatial;
// Materials of the Blender-made building models (R45, art/buildings). UV is metres on each
// surface (walls: along and up; roofs: along the eaves and up the slope); glass, shutters and
// doors span 0–1. `pattern` picks the material. Buildings vary per instance: the instance
// colour (COLOR) tints plaster, INSTANCE_CUSTOM.rgb tints roofs, and INSTANCE_CUSTOM.a picks
// shutter colours and whether a building has shutters and flowers at all. Patterns fade to
// their average where they would be smaller than a pixel.

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

uniform int pattern = 0;
uniform vec3 color : source_color = vec3(0.8);
uniform float roughness = 0.9;
uniform float metallic = 0.0;
uniform float specular = 0.4;

// Shutter colours: dark green, grey-green, oxblood, brown, grey (sRGB).
const vec3 SHUTTERS[5] = {
	vec3(0.2, 0.32, 0.22), vec3(0.42, 0.5, 0.44), vec3(0.45, 0.13, 0.1), vec3(0.36, 0.23, 0.15),
	vec3(0.56, 0.57, 0.57)
};

varying vec3 roof_tint;
varying float variant;
varying vec3 world_position;

float hash(vec2 p) {
	return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float noise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);
	vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(
		mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
		mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x),
		u.y
	);
}

// 1 inside [from, to], edges softened over `soft`.
float band(float x, float from, float to, float soft) {
	return smoothstep(from - soft, from + soft, x) * (1.0 - smoothstep(to - soft, to + soft, x));
}

vec3 linear(vec3 srgb) {
	return pow(srgb, vec3(2.2));
}

void vertex() {
	roof_tint = INSTANCE_CUSTOM.rgb;
	variant = INSTANCE_CUSTOM.a;
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}

void fragment() {
	vec2 uv = UV;
	float soft = max(max(fwidth(uv.x), fwidth(uv.y)) * 0.5, 1e-4);
	float detail = 1.0 - smoothstep(0.08, 0.35, soft * 2.0);
	vec3 base = linear(color);
	vec3 albedo = base;
	vec3 average = base;
	float rough = roughness;
	float bump = 0.0;
	// Tangent-space normal tilt from the pattern's relief.
	vec2 slope = vec2(0.0);

	if (pattern == PLASTER) {
		base = linear(COLOR.rgb);
		float grain = noise(uv * 9.0) * 0.5 + noise(uv * 2.3) * 0.5;
		albedo = base * (0.93 + 0.1 * grain) * (0.96 + 0.06 * noise(uv * 0.4));
		// Weathering: rain darkens walls towards the bottom in long streaks.
		albedo *= 1.0 - 0.06 * noise(vec2(uv.x * 1.3, uv.y * 0.15));
		average = base * 0.97;
		slope = (vec2(noise(uv * 31.0), noise(uv * 31.0 + 7.0)) - 0.5) * 0.25;
	} else if (pattern == STONE) {
		// Squared stone or concrete in courses.
		float course = uv.y / 0.4;
		float block = uv.x / 0.75 + 0.5 * mod(floor(course), 2.0);
		float joint = 1.0 - band(fract(course), 0.04, 0.96, soft / 0.4)
			* band(fract(block), 0.03, 0.97, soft / 0.75);
		float tone = 0.85 + 0.3 * hash(vec2(floor(block), floor(course)));
		albedo = base * tone * mix(1.0, 0.6, joint) * (0.9 + 0.2 * noise(uv * 5.0));
		average = base * 0.88;
	} else if (pattern == WOOD || pattern == WOOD_DARK) {
		// Horizontal boards with grain; dark wood is beams and rafters.
		float boards = uv.y / 0.16;
		float row = floor(boards);
		float joint = 1.0 - band(fract(boards), 0.06, 0.94, soft / 0.16);
		float tone = 0.82 + 0.3 * hash(vec2(row, floor(uv.x / 3.1 + hash(vec2(row, 1.0)))));
		float grain = noise(vec2(uv.x * 1.5, uv.y * 40.0));
		albedo = base * tone * (0.85 + 0.25 * grain) * mix(1.0, 0.5, joint);
		average = base * 0.86;
		slope.y = (smoothstep(0.0, 0.1, fract(boards)) - smoothstep(0.9, 1.0, fract(boards))) * 0.4;
	} else if (pattern == GLASS) {
		// Panes reflect the sky; a curtain or the room behind shows faintly.
		float curtain = step(0.5, hash(floor(world_position.xz * 0.7)))
			* band(uv.x, 0.0, 0.28, 0.02) * (1.0 - step(0.95, uv.y));
		albedo = mix(base, linear(vec3(0.75, 0.72, 0.66)), curtain * 0.5);
		average = albedo;
		rough = 0.06;
	} else if (pattern == SHUTTER) {
		// Louvred shutters in the building's colour; some buildings have none.
		if (variant < 0.25) {
			discard;
		}
		vec3 paint = linear(SHUTTERS[int(variant * 97.0) % 5]);
		float slat = fract(uv.y * 18.0);
		float frame_edge = 1.0 - band(uv.x, 0.08, 0.92, 0.01) * band(uv.y, 0.05, 0.95, 0.01);
		albedo = paint * mix(mix(0.7, 1.05, smoothstep(0.0, 0.8, slat)), 1.0, frame_edge);
		average = paint * 0.9;
		slope.y = mix(-0.6, 0.0, frame_edge);
	} else if (pattern == DOOR) {
		// A wooden door: four panels in a frame.
		float panel = band(uv.x, 0.14, 0.46, 0.01) + band(uv.x, 0.54, 0.86, 0.01);
		panel *= band(uv.y, 0.1, 0.45, 0.01) + band(uv.y, 0.55, 0.9, 0.01);
		albedo = base * mix(0.8, 1.0, panel) * (0.9 + 0.2 * noise(vec2(uv.x * 4.0, uv.y * 30.0)));
		average = base * 0.9;
	} else if (pattern == TILES || pattern == SLATE) {
		// Rows of tiles up the slope, staggered; each row's lower edge shades the one below.
		bool slate = pattern == SLATE;
		vec3 tint = roof_tint.r + roof_tint.g + roof_tint.b > 0.0 && !slate ? linear(roof_tint) : base;
		float row_height = slate ? 0.24 : 0.32;
		float tile_width = slate ? 0.3 : 0.21;
		float rows = uv.y / row_height;
		float row = floor(rows);
		float columns = uv.x / tile_width + 0.5 * mod(row, 2.0);
		float tone = 0.8 + 0.4 * hash(vec2(floor(columns), row));
		float in_row = fract(rows);
		float overlap = mix(0.5, 1.0, smoothstep(0.0, 0.3, in_row));
		float gap = 1.0 - band(fract(columns), 0.03, 0.97, soft / tile_width);
		albedo = tint * tone * overlap * mix(1.0, 0.65, gap);
		// Moss and dirt in large patches.
		albedo *= 0.82 + 0.3 * noise(uv * 0.45);
		average = tint * 0.8;
		slope.y = -0.35 + 0.9 * (1.0 - smoothstep(0.0, 0.12, in_row));
		slope.x = (slate ? 0.0 : 0.5) * sin(fract(columns) * 6.2832);
	} else if (pattern == COPPER) {
		albedo = base * (0.85 + 0.3 * noise(uv * 2.0));
		average = base;
	} else if (pattern == FLOWERS) {
		// Geraniums: red, pink or white per box.
		float which = hash(floor(world_position.xz * 0.5));
		albedo = which < 0.6 ? linear(vec3(0.85, 0.08, 0.1)) : which < 0.85 ? linear(vec3(0.92, 0.35, 0.55)) : linear(vec3(0.93, 0.92, 0.9));
		average = albedo;
		if (variant > 0.65) {
			discard;
		}
	} else if (pattern == LEAVES) {
		albedo = base * (0.8 + 0.4 * noise(world_position.xz * 9.0 + world_position.y * 5.0));
		average = base;
		if (variant > 0.65) {
			discard;
		}
	} else if (pattern == CLOCK) {
		// A dark dial with gilded marks and hands at ten past ten.
		vec2 centred = (uv - 0.5) * 2.0;
		float radius = length(centred);
		float angle = atan(centred.x, centred.y);
		float hour = abs(fract(angle / 6.2832 * 12.0 + 0.5) - 0.5);
		float marks = step(0.66, radius) * step(radius, 0.82) * step(hour, 0.035);
		float ring = step(0.88, radius);
		float hands = 0.0;
		for (int i = 0; i < 2; i++) {
			float turn = i == 0 ? 0.847 : 0.167;
			float reach = i == 0 ? 0.5 : 0.75;
			vec2 direction = vec2(sin(turn * 6.2832), cos(turn * 6.2832));
			float along = clamp(dot(centred, direction), 0.0, reach);
			hands = max(hands, step(length(centred - direction * along), 0.045));
		}
		vec3 gold = linear(vec3(0.83, 0.66, 0.3));
		albedo = mix(base, gold, max(max(marks, ring), hands));
		average = mix(base, gold, 0.3);
	} else if (pattern == SHEET) {
		// Sheet metal: standing seams down the slope, streaked by weather.
		float seam = 1.0 - band(fract(uv.x / 0.52) * 0.52, 0.025, 0.495, soft);
		albedo = base * mix(1.0, 1.35, seam) * (0.9 + 0.2 * noise(vec2(uv.x * 2.0, uv.y * 0.25)));
		average = base * 1.04;
		slope.x = seam * 0.8 * sign(fract(uv.x / 0.52) - 0.5);
	} else if (pattern == GARAGE) {
		// A sectional garage door: horizontal panels with grooves.
		float groove = 1.0 - band(fract(uv.y * 4.0), 0.04, 0.96, 0.01);
		albedo = base * mix(1.0, 0.6, groove) * (0.95 + 0.05 * noise(uv * 40.0));
		average = base * 0.92;
		slope.y = groove * 0.6;
	} else {
		// Frames and metal: plain.
		albedo = base;
	}

	ALBEDO = mix(average, albedo, detail);
	ROUGHNESS = rough;
	METALLIC = metallic;
	SPECULAR = specular;
	NORMAL_MAP = normalize(vec3(-slope * detail, 1.0)) * 0.5 + 0.5;
}
