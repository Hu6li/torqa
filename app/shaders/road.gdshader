shader_type spatial;
// The road ridden (R45, ADR 0011): flat asphalt in the palette's blue-grey with cream markings —
// solid edge lines and a dashed centre line, crisp but anti-aliased so they hold in the distance —
// and plain shoulders on the skirts below its edges. Faceted like the ground. UV.x runs 0–1
// across the road (beyond, the skirts), UV.y is the distance along the route in metres.

uniform vec3 asphalt_color : source_color = vec3(0.557, 0.584, 0.608);
uniform vec3 marking_color : source_color = vec3(0.949, 0.922, 0.851);
uniform vec3 shoulder_color : source_color = vec3(0.765, 0.733, 0.706);
uniform float road_width = 6.0;
uniform float line_width = 0.12;
uniform float dash_length = 3.0;
uniform float dash_gap = 6.0;
// 0 dry, 1 soaked: darker asphalt with a soft sheen.
uniform float wetness : hint_range(0.0, 1.0) = 0.0;

// 1 inside [from, to] across the road, with edges one pixel soft.
float band(float across, float from, float to) {
	float soft = max(fwidth(across), 1e-4);
	return smoothstep(from - soft, from + soft, across) * (1.0 - smoothstep(to - soft, to + soft, across));
}

void fragment() {
	NORMAL = normalize(cross(dFdy(VERTEX), dFdx(VERTEX)));
	float across = UV.x * road_width;
	float along = UV.y;
	float edges = band(across, 0.25, 0.25 + line_width)
		+ band(across, road_width - 0.25 - line_width, road_width - 0.25);
	float half_line = line_width * 0.5;
	float centre_line = band(across, road_width * 0.5 - half_line, road_width * 0.5 + half_line);
	float dash = mod(along, dash_length + dash_gap);
	float soft = max(fwidth(along), 1e-4);
	centre_line *= 1.0 - smoothstep(dash_length - soft, dash_length + soft, dash);
	float marking = clamp(edges + centre_line, 0.0, 1.0);
	vec3 albedo = mix(asphalt_color * (1.0 - wetness * 0.25), marking_color, marking);
	float shoulder = 1.0 - step(0.0, UV.x) * step(UV.x, 1.0);
	ALBEDO = mix(albedo, shoulder_color, shoulder);
	ROUGHNESS = mix(0.9, 0.55, wetness * (1.0 - shoulder));
	SPECULAR = mix(0.15, 0.35, wetness);
}
