shader_type spatial;
// Raindrops (#103, ADR 0011): slim faceted diamonds in a pale sky colour, lit like the world and
// a little see-through; the particles align them with their fall.
render_mode cull_disabled, depth_draw_never;

uniform vec3 color : source_color = vec3(0.89, 0.929, 0.941);
uniform float opacity : hint_range(0.0, 1.0) = 0.55;

void fragment() {
	NORMAL = normalize(cross(dFdy(VERTEX), dFdx(VERTEX)));
	ALBEDO = color;
	ALPHA = opacity;
	ROUGHNESS = 0.35;
	SPECULAR = 0.5;
}
