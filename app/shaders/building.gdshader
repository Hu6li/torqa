shader_type spatial;
// Buildings: vertex-coloured façades (sRGB) with windows per storey and a darker plinth;
// roofs are marked by vertex alpha 1. UV.x runs along the outline in metres, UV.y is the
// height above the ground in metres.

uniform float storey = 3.0;
uniform float window_spacing = 3.2;
uniform vec3 glass : source_color = vec3(0.16, 0.2, 0.24);
uniform vec3 frame : source_color = vec3(0.95, 0.95, 0.93);

void fragment() {
	vec3 base = pow(COLOR.rgb, vec3(2.2));
	float roof = step(0.5, COLOR.a);
	vec2 wall = UV;

	float row = fract(wall.y / storey);
	float column = fract(wall.x / window_spacing);
	float above_plinth = step(1.0, wall.y);
	float pane = step(0.35, row) * step(row, 0.78) * step(0.3, column) * step(column, 0.7);
	float casing = step(0.31, row) * step(row, 0.82) * step(0.26, column) * step(column, 0.74);
	float walls = 1.0 - roof;
	pane *= above_plinth * walls;
	casing *= above_plinth * walls;

	// Far away, windows shrink below a pixel and alias; blend them into their average tone.
	float detail = 1.0 - smoothstep(0.2, 0.6, max(fwidth(wall.x), fwidth(wall.y)));
	vec3 average = mix(base, pow(glass, vec3(2.2)), 0.18 * above_plinth * walls);
	vec3 color = mix(base, pow(frame, vec3(2.2)), casing - pane);
	color = mix(color, pow(glass, vec3(2.2)), pane);
	color = mix(average, color, detail);
	pane *= detail;
	// Plinth and ground contact read darker, as on real façades.
	color *= mix(0.72, 1.0, smoothstep(0.0, 0.9, wall.y) * walls + roof);

	ALBEDO = color;
	ROUGHNESS = mix(0.88, 0.12, pane);
	SPECULAR = mix(0.3, 0.75, pane);
}
