shader_type spatial;
// Flat-coloured, faceted surfaces (ADR 0011): one normal per face, derived on screen, so any
// simple mesh reads as low-poly whatever normals it carries.

uniform vec3 albedo : source_color = vec3(0.7);

void fragment() {
	NORMAL = normalize(cross(dFdy(VERTEX), dFdx(VERTEX)));
	ALBEDO = albedo;
	ROUGHNESS = 0.9;
	SPECULAR = 0.15;
}
