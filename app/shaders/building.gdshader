shader_type spatial;
// Buildings (R45). Each surface's style arrives in the vertex colour's alpha as code / 9 and
// its base colour in RGB (sRGB); the codes are those of `Style` in
// core/torqa-world/src/buildings/parts.rs. Walls carry metres along the wall and above the
// ground in UV, roofs metres along the eaves and up the slope, single windows 0–1 across.
// Patterns fade to their average colour where they would be smaller than a pixel, so they
// never shimmer in the distance.

uniform float storey = 3.0;
uniform float window_spacing = 3.2;
uniform vec3 glass : source_color = vec3(0.16, 0.2, 0.24);
uniform vec3 frame : source_color = vec3(0.95, 0.95, 0.93);

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

// Shutter colours: dark green, grey-green, brown, grey (sRGB).
const vec3 SHUTTERS[4] = { vec3(0.2, 0.32, 0.22), vec3(0.42, 0.5, 0.44), vec3(0.36, 0.23, 0.15), vec3(0.56, 0.57, 0.57) };

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

void fragment() {
	int style = int(round(COLOR.a * 9.0));
	vec3 base = linear(COLOR.rgb);
	vec2 uv = UV;
	float soft = max(max(fwidth(uv.x), fwidth(uv.y)) * 0.5, 1e-4);
	// Where patterns would be smaller than a pixel, their average colour shows instead.
	float detail = 1.0 - smoothstep(0.12, 0.45, soft * 2.0);
	vec3 color = base;
	vec3 average = base;
	float roughness = 0.9;
	float specular = 0.3;
	float metallic = 0.0;
	// The building's own variation: its colour differs a little from its neighbours'.
	float pick = hash(COLOR.rg * 251.0 + COLOR.b * 17.0);
	bool wall = style <= CLADDING;

	if (style == PLASTER || style == TIMBER) {
		if (style == TIMBER) {
			// Horizontal boards, each a slightly different tone, with dark joints.
			float board = uv.y / 0.2;
			float joint = 1.0 - band(fract(board) * 0.2, 0.012, 0.188, soft);
			float row = floor(board);
			float tone = 0.86 + 0.28 * hash(vec2(row, floor(uv.x / 4.0 + hash(vec2(row, 3.0)))));
			color = base * tone * mix(1.0, 0.55, joint);
			color *= 0.92 + 0.16 * noise(vec2(uv.x * 0.7, uv.y * 30.0));
			average = base * 0.93;
		} else {
			color = base * (0.94 + 0.12 * noise(uv * 0.8));
		}
		// A grid of windows, from the ground floor up: frame, glass, a sill below, and on
		// some buildings shutters beside them.
		float in_row = fract(uv.y / storey) * storey;
		float in_column = fract(uv.x / window_spacing) * window_spacing;
		float lowest = step(1.0, uv.y);
		float pane = band(in_row, 1.05, 2.34, soft) * band(in_column, 0.96, 2.24, soft) * lowest;
		float casing = band(in_row, 0.95, 2.44, soft) * band(in_column, 0.86, 2.34, soft) * lowest;
		float sill = band(in_row, 0.86, 0.95, soft) * band(in_column, 0.8, 2.4, soft) * lowest;
		float shutters = step(0.45, pick) * band(in_row, 0.95, 2.44, soft) * lowest
			* (band(in_column, 0.32, 0.86, soft) + band(in_column, 2.34, 2.88, soft));
		vec3 shutter = linear(SHUTTERS[int(pick * 7.0) % 4]);
		// Panes show the sky dimly, with a lighter reflection at the top.
		vec3 pane_color = linear(glass) * (0.8 + 0.5 * smoothstep(1.4, 2.3, in_row));
		color = mix(color, linear(frame), casing - pane);
		color = mix(color, linear(vec3(0.75, 0.74, 0.7)), sill);
		color = mix(color, shutter * (0.9 + 0.1 * step(0.5, fract(in_row * 12.0))), shutters);
		color = mix(color, pane_color, pane);
		average = mix(average, linear(glass), 0.15 * lowest);
		pane *= detail;
		roughness = mix(0.9, 0.1, pane);
		specular = mix(0.3, 0.8, pane);
	} else if (style == BLANK) {
		color = base * (0.94 + 0.12 * noise(uv * 0.8));
	} else if (style == BOARDS) {
		float board = uv.y / 0.2;
		float joint = 1.0 - band(fract(board) * 0.2, 0.012, 0.188, soft);
		float row = floor(board);
		color = base * (0.86 + 0.28 * hash(vec2(row, floor(uv.x / 4.0 + hash(vec2(row, 3.0))))));
		color *= mix(1.0, 0.55, joint);
		average = base * 0.93;
	} else if (style == CHURCH) {
		// Tall windows with round arches and stone surrounds, every 4.5 m.
		color = base * (0.95 + 0.1 * noise(uv * 0.6));
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
		// Leaded glass: small panes of slightly different tints.
		vec2 lead = vec2(x / 0.18, uv.y / 0.24);
		vec3 stained = linear(glass) * (0.7 + 0.6 * hash(floor(lead)));
		color = mix(color, linear(vec3(0.74, 0.72, 0.68)), surround - pane);
		color = mix(color, stained, pane);
		average = mix(base, linear(glass), 0.12);
		roughness = mix(0.9, 0.2, pane * detail);
	} else if (style == CLADDING) {
		// Profiled metal sheets, a concrete plinth and a band of windows high up.
		float rib = fract(uv.x / 0.25);
		color = base * (0.86 + 0.14 * smoothstep(0.0, 0.5, abs(rib - 0.5)));
		float plinth = 1.0 - smoothstep(0.5 - soft, 0.5 + soft, uv.y);
		float strip = band(uv.y, 3.1, 3.9, soft);
		float mullion = 1.0 - band(fract(uv.x / 1.6) * 1.6, 0.05, 1.55, soft);
		color = mix(color, linear(vec3(0.55, 0.55, 0.53)), plinth);
		color = mix(color, linear(glass), strip * (1.0 - mullion));
		color = mix(color, linear(vec3(0.3, 0.31, 0.32)), strip * mullion);
		average = base * 0.93;
		metallic = 0.3 * (1.0 - plinth) * (1.0 - strip);
		roughness = mix(0.55, 0.15, strip * detail);
		specular = mix(0.45, 0.8, strip * detail);
	} else if (style == WINDOW) {
		// UV spans the window: a frame round the glass, divided into four.
		float border = max(
			1.0 - band(uv.x, 0.12, 0.88, soft),
			1.0 - band(uv.y, 0.1, 0.9, soft)
		);
		float cross = max(band(uv.x, 0.47, 0.53, soft), band(uv.y, 0.57, 0.62, soft));
		float pane = (1.0 - border) * (1.0 - cross);
		color = mix(base, linear(glass) * (0.8 + 0.5 * smoothstep(0.4, 0.9, uv.y)), pane);
		average = mix(base, linear(glass), 0.6);
		roughness = mix(0.8, 0.1, pane);
		specular = mix(0.3, 0.8, pane);
	} else if (style == TILES) {
		// Rows of tiles up the slope, staggered, each row shading the top of the one below.
		float rows = uv.y / 0.32;
		float row = floor(rows);
		float columns = uv.x / 0.24 + 0.5 * mod(row, 2.0);
		float tone = 0.82 + 0.36 * hash(vec2(floor(columns), row));
		float overlap = mix(0.55, 1.0, smoothstep(0.0, 0.25, fract(rows)));
		float gap = 1.0 - band(fract(columns), 0.04, 0.96, soft / 0.24);
		color = base * tone * overlap * mix(1.0, 0.7, gap);
		// Weathering in large patches.
		color *= 0.85 + 0.3 * noise(uv * 0.35);
		average = base * 0.82;
		roughness = 0.8;
		specular = 0.35;
	} else if (style == SHEET) {
		// Standing seams down the slope.
		float seam = 1.0 - band(fract(uv.x / 0.55) * 0.55, 0.03, 0.52, soft);
		color = base * mix(1.0, 1.3, seam) * (0.92 + 0.16 * noise(vec2(uv.x * 2.0, uv.y * 0.3)));
		average = base * 1.03;
		metallic = 0.4;
		roughness = 0.45;
		specular = 0.5;
	} else {
		// Gravel or membrane.
		color = base * (0.8 + 0.4 * noise(uv * 3.0)) * (0.9 + 0.2 * noise(uv * 0.3));
		average = base;
		roughness = 0.95;
		specular = 0.2;
	}

	color = mix(average, color, detail);
	if (wall) {
		// Darker where walls meet the ground.
		color *= mix(0.72, 1.0, smoothstep(0.0, 0.9, uv.y));
	}
	ALBEDO = color;
	ROUGHNESS = roughness;
	SPECULAR = specular;
	METALLIC = metallic;
}
