shader_type spatial;
#include "res://shaders/winter.gdshaderinc"

varying vec3 winter_position;
varying vec3 winter_normal;

void vertex() {
	winter_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	winter_normal = normalize(MODEL_NORMAL_MATRIX * NORMAL);
}
// Building shells (R89, ADR 0020): buildings drawn by the world itself — far away, and up close
// where no model fits. Walls of textured plaster or boards, roofs of clay tiles or gravel
// (textures baked in Blender, tinted by the surface's palette colour), with window rectangles
// of dark, reflective glass. Each surface's style arrives in the vertex colour's alpha as code / 11 and its colour in
// RGB (sRGB); the codes are those of `Style` in core/orqwa-world/src/buildings/parts.rs. Walls
// carry metres along the wall and above the ground in UV, roofs metres along the eaves and up
// the slope, single windows 0–1 across. Windows give way to their average colour where they
// would be smaller than a pixel, so they never shimmer in the distance.

uniform float storey = 3.0;
uniform float window_spacing = 3.2;
uniform vec3 glass : source_color = vec3(0.384, 0.514, 0.533);
uniform vec3 frame : source_color = vec3(0.949, 0.922, 0.851);
uniform vec3 stained_glass : source_color = vec3(0.435, 0.424, 0.62);
uniform vec3 stone : source_color = vec3(0.765, 0.733, 0.706);
uniform sampler2D plaster_texture : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D boards_texture : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D tiles_texture : source_color, filter_linear_mipmap_anisotropic, repeat_enable;
uniform sampler2D gravel_texture : source_color, filter_linear_mipmap_anisotropic, repeat_enable;

const int PLASTER = 0;
const int BLANK = 1;
const int TIMBER = 2;
const int BOARDS = 3;
const int CHURCH = 4;
const int CLADDING = 5;
const int WINDOW = 6;
const int TILES = 7;
const int SHEET = 8;
const int FLAT = 9;
const int SHOP = 10;
const int RIBBON = 11;

// 1 inside [from, to], edges softened over `soft`.
float band(float x, float from, float to, float soft) {
	return smoothstep(from - soft, from + soft, x) * (1.0 - smoothstep(to - soft, to + soft, x));
}

vec3 to_linear(vec3 srgb) {
	return mix(srgb / 12.92, pow((srgb + 0.055) / 1.055, vec3(2.4)), step(0.04045, srgb));
}

void fragment() {
	int style = int(round(COLOR.a * 11.0));
	vec2 uv = UV;
	// The surface's texture, light and neutral, tinted by its colour.
	vec3 surface = vec3(0.8);
	if (style == TILES) {
		surface = texture(tiles_texture, uv / 2.0).rgb;
	} else if (style == FLAT) {
		surface = texture(gravel_texture, uv / 1.5).rgb * 1.6;
	} else if (style == BOARDS || style == TIMBER) {
		surface = texture(boards_texture, uv / 2.0).rgb * 1.8;
	} else if (style != WINDOW && style != SHEET) {
		surface = texture(plaster_texture, uv / 2.0).rgb;
	}
	vec3 base = to_linear(COLOR.rgb) * surface * 1.25;
	float soft = max(max(fwidth(uv.x), fwidth(uv.y)) * 0.5, 1e-4);
	// Where windows would be smaller than a pixel, their average colour shows instead.
	float detail = 1.0 - smoothstep(0.12, 0.45, soft * 2.0);
	vec3 color = base;
	vec3 average = base;
	float roughness = 0.9;
	float specular = 0.2;
	bool wall = style <= CLADDING || style == RIBBON;

	if (style == PLASTER || style == TIMBER) {
		// A grid of windows from the ground floor up: a frame round the glass.
		float in_row = fract(uv.y / storey) * storey;
		float in_column = fract(uv.x / window_spacing) * window_spacing;
		float lowest = step(1.0, uv.y);
		float pane = band(in_row, 1.05, 2.34, soft) * band(in_column, 0.96, 2.24, soft) * lowest;
		float casing = band(in_row, 0.95, 2.44, soft) * band(in_column, 0.86, 2.34, soft) * lowest;
		color = mix(color, frame, casing - pane);
		color = mix(color, glass * 0.55, pane);
		average = mix(base, glass * 0.55, 0.15 * lowest);
		roughness = mix(0.9, 0.05, pane * detail);
		specular = mix(0.3, 0.5, pane);
	} else if (style == CHURCH) {
		// Tall windows with round arches and stone surrounds, every 4.5 m.
		float x = mod(uv.x, 4.5) - 2.25;
		float half_width = 0.55;
		float spring = 6.0 - half_width;
		float arch = 1.0 - smoothstep(half_width - soft, half_width + soft, length(vec2(x, uv.y - spring)));
		float pane = max(
			band(x, -half_width, half_width, soft) * band(uv.y, 2.4, spring, soft),
			arch * step(spring, uv.y)
		);
		float outer = half_width + 0.16;
		float surround = max(
			band(x, -outer, outer, soft) * band(uv.y, 2.24, spring, soft),
			(1.0 - smoothstep(outer - soft, outer + soft, length(vec2(x, uv.y - spring))))
				* step(spring, uv.y)
		);
		color = mix(color, stone, surround - pane);
		color = mix(color, stained_glass, pane);
		average = mix(base, stained_glass, 0.12);
	} else if (style == CLADDING) {
		// A plinth and a band of windows high up.
		float plinth = 1.0 - smoothstep(0.5 - soft, 0.5 + soft, uv.y);
		float strip = band(uv.y, 3.1, 3.9, soft);
		color = mix(color, stone, plinth);
		color = mix(color, glass, strip);
		average = base;
	} else if (style == SHOP) {
		// A shop front: in each bay a big pane over a low base, a fascia above; the frame
		// in the shop's colour (`base`).
		float in_column = fract(uv.x / window_spacing) * window_spacing;
		float pane = band(in_column, 0.18, window_spacing - 0.18, soft) * band(uv.y, 0.45, 2.55, soft);
		color = mix(base, glass, pane);
		average = mix(base, glass, 0.55);
		roughness = mix(0.9, 0.05, pane);
	} else if (style == RIBBON) {
		// Offices: a band of glass along every storey from the first floor up, framed above
		// and below, between plain spandrels.
		float in_row = fract(uv.y / storey) * storey;
		float upper = step(storey, uv.y);
		float pane = band(in_row, 0.95, 2.35, soft) * upper;
		float casing = band(in_row, 0.85, 2.45, soft) * upper;
		color = mix(color, frame, casing - pane);
		color = mix(color, glass * 0.55, pane);
		average = mix(base, glass * 0.55, 0.45 * upper);
		roughness = mix(0.9, 0.05, pane);
	} else if (style == WINDOW) {
		// UV spans the window: a frame round the glass.
		float pane = band(uv.x, 0.12, 0.88, soft) * band(uv.y, 0.1, 0.9, soft);
		color = mix(base, glass, pane);
		average = mix(base, glass, 0.6);
		roughness = mix(0.9, 0.05, pane);
	}
	// Blank walls, boards and roofs (tiles, sheet, flat) are their textured colour alone.

	color = mix(average, color, detail);
	if (wall) {
		// A soft shade where walls meet the ground.
		color *= mix(0.72, 1.0, smoothstep(0.0, 1.4, uv.y));
	}
	float snow = wall ? 0.0 : winter_cover(winter_position, winter_normal);
	ALBEDO = mix(color, winter_snow_color.rgb, snow);
	roughness = mix(roughness, 0.8, snow);
	ROUGHNESS = roughness;
	SPECULAR = specular;
}
