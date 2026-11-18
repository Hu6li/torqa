// Opaque window interiors retain sky reflections without transparent surfaces or room meshes.
uniform vec3 interior_dark : source_color;
uniform vec3 interior_light : source_color;
uniform vec3 curtain_color : source_color;

// A home: most panes show a dim room, deeper at the sides and lighter towards the ceiling; some
// have drawn-back curtains, a blind partly down or a sheer net. Each kind is a minority, as on
// a real street, and curtains vary in shade so neighbours do not match.
vec3 building_interior(vec2 pane, float seed) {
	pane = clamp(pane, vec2(0.0), vec2(1.0));
	float side = min(pane.x, 1.0 - pane.x);
	vec3 room = mix(interior_dark, interior_light, fract(seed * 7.31)) * 0.5;
	room *= mix(0.72, 1.0, smoothstep(0.0, 0.3, side)) * mix(0.85, 1.05, pane.y);
	vec3 fabric = curtain_color * mix(0.55, 0.85, fract(seed * 3.17));
	float fine = 1.0 - smoothstep(0.015, 0.05, fwidth(pane.x));
	float pleat = 0.93 + fine * 0.07 * sin(pane.x * 95.0);
	if (seed < 0.45) {
		return room;
	} else if (seed < 0.72) {
		float width = 0.1 + fract(seed * 13.7) * 0.1;
		float curtains = 1.0 - smoothstep(width, width + 0.05, side);
		return mix(room, fabric * pleat, curtains);
	} else if (seed < 0.88) {
		float down = 0.6 + fract(seed * 5.3) * 0.35;
		float blind = smoothstep(down, down + 0.01, pane.y);
		float slats = 0.92 + fine * 0.08 * step(0.5, fract(pane.y * 40.0));
		return mix(room, fabric * 0.9 * slats, blind);
	}
	// A sheer net lets the room through, lighter low down where it hangs.
	return mix(room, fabric * pleat, 0.45 - 0.15 * pane.y);
}

// A shop: a lit sales floor with no curtains: shelves of goods in the light, darker towards the
// sides and the floor, and a bright band of ceiling lights.
vec3 shop_interior(vec2 pane, float seed) {
	pane = clamp(pane, vec2(0.0), vec2(1.0));
	vec3 lit = mix(interior_light, vec3(1.0, 0.92, 0.78), 0.4) * mix(0.5, 0.7, seed);
	float side = min(pane.x, 1.0 - pane.x);
	vec3 back = lit * mix(0.4, 0.8, smoothstep(0.0, 0.3, side)) * mix(0.6, 1.0, pane.y);
	float columns = 2.0 + floor(seed * 3.0);
	float within = fract(pane.x * columns);
	float shelf = step(0.15, within) * step(within, 0.85) * step(0.06, pane.y) * step(pane.y, 0.6);
	float row = floor(pane.y * 8.0);
	float goods_seed = fract(sin((floor(pane.x * columns) + row * 7.0 + seed * 31.0) * 12.9898) * 43758.5453);
	vec3 goods = mix(interior_dark, mix(interior_light, curtain_color, goods_seed), 0.5 + 0.4 * goods_seed);
	goods *= mix(0.55, 0.95, goods_seed) * (0.8 + 0.2 * step(0.15, fract(pane.y * 8.0)));
	vec3 inside = mix(back, goods, shelf);
	float lights = smoothstep(0.86, 0.9, pane.y) * (1.0 - smoothstep(0.95, 0.99, pane.y));
	return mix(inside, lit * 1.6, lights * 0.8);
}
