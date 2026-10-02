shader_type spatial;
// Asphalt with solid edge lines and a dashed centre line. UV.x runs 0–1 across the road, UV.y
// is the distance along the route in metres.

uniform vec3 asphalt_color : source_color = vec3(0.16, 0.16, 0.17);
uniform vec3 marking_color : source_color = vec3(0.92, 0.92, 0.88);
uniform float road_width = 6.0;
uniform float line_width = 0.12;
uniform float dash_length = 3.0;
uniform float dash_gap = 6.0;

float hash(vec2 p) {
	return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

void fragment() {
	float across = UV.x * road_width;
	float half_line = line_width * 0.5;
	bool edge = across < 0.25 + line_width && across > 0.25
		|| across > road_width - 0.25 - line_width && across < road_width - 0.25;
	bool centre = abs(across - road_width * 0.5) < half_line
		&& mod(UV.y, dash_length + dash_gap) < dash_length;
	float grain = hash(floor(vec2(across, UV.y) * 20.0)) * 0.04;
	ALBEDO = edge || centre ? marking_color : asphalt_color + grain;
	ROUGHNESS = edge || centre ? 0.6 : 0.85;
}
