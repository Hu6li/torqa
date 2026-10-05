shader_type spatial;
// Building shells (R45, ADR 0011): buildings drawn by the world itself — far away, and up close
// where no model fits. Flat palette colours, faceted, with clean window rectangles and nothing
// finer. Each surface's style arrives in the vertex colour's alpha as code / 9 and its colour in
// RGB (sRGB); the codes are those of `Style` in core/torqa-world/src/buildings/parts.rs. Walls
// carry metres along the wall and above the ground in UV, roofs metres along the eaves and up
// the slope, single windows 0–1 across. Windows give way to their average colour where they
// would be smaller than a pixel, so they never shimmer in the distance.

uniform float storey = 3.0;
uniform float window_spacing = 3.2;
uniform vec3 glass : source_color = vec3(0.384, 0.514, 0.533);
uniform vec3 frame : source_color = vec3(0.949, 0.922, 0.851);
uniform vec3 stained_glass : source_color = vec3(0.435, 0.424, 0.62);
uniform vec3 stone : source_color = vec3(0.765, 0.733, 0.706);

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

// 1 inside [from, to], edges softened over `soft`.
float band(float x, float from, float to, float soft) {
	return smoothstep(from - soft, from + soft, x) * (1.0 - smoothstep(to - soft, to + soft, x));
}

vec3 to_linear(vec3 srgb) {
	return mix(srgb / 12.92, pow((srgb + 0.055) / 1.055, vec3(2.4)), step(0.04045, srgb));
}

void fragment() {
	NORMAL = normalize(cross(dFdy(VERTEX), dFdx(VERTEX)));
	int style = int(round(COLOR.a * 9.0));
	vec3 base = to_linear(COLOR.rgb);
	vec2 uv = UV;
	float soft = max(max(fwidth(uv.x), fwidth(uv.y)) * 0.5, 1e-4);
	// Where windows would be smaller than a pixel, their average colour shows instead.
	float detail = 1.0 - smoothstep(0.12, 0.45, soft * 2.0);
	vec3 color = base;
	vec3 average = base;
	float roughness = 0.9;
	float specular = 0.2;
	bool wall = style <= CLADDING;

	if (style == PLASTER || style == TIMBER) {
		// A grid of windows from the ground floor up: a frame round the glass.
		float in_row = fract(uv.y / storey) * storey;
		float in_column = fract(uv.x / window_spacing) * window_spacing;
		float lowest = step(1.0, uv.y);
		float pane = band(in_row, 1.05, 2.34, soft) * band(in_column, 0.96, 2.24, soft) * lowest;
		float casing = band(in_row, 0.95, 2.44, soft) * band(in_column, 0.86, 2.34, soft) * lowest;
		color = mix(color, frame, casing - pane);
		color = mix(color, glass, pane);
		average = mix(base, glass, 0.15 * lowest);
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
	} else if (style == WINDOW) {
		// UV spans the window: a frame round the glass.
		float pane = band(uv.x, 0.12, 0.88, soft) * band(uv.y, 0.1, 0.9, soft);
		color = mix(base, glass, pane);
		average = mix(base, glass, 0.6);
		roughness = mix(0.9, 0.35, pane);
	}
	// Blank walls, boards and roofs (tiles, sheet, flat) are their colour alone.

	color = mix(average, color, detail);
	if (wall) {
		// A soft shade where walls meet the ground.
		color *= mix(0.85, 1.0, smoothstep(0.0, 1.2, uv.y));
	}
	ALBEDO = color;
	ROUGHNESS = roughness;
	SPECULAR = specular;
}
