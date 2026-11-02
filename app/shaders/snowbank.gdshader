shader_type spatial;
render_mode cull_disabled;

#include "res://shaders/ground.gdshaderinc"

uniform vec4 snow_color : source_color;
varying vec3 world_position;

void vertex() {
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}

void fragment() {
	float grain = ground_noise(world_position.xz * 30.0);
	float drift = ground_noise(world_position.xz * 1.7);
	ALBEDO = snow_color.rgb * mix(0.87, 1.0, drift) * mix(0.96, 1.0, grain);
	ROUGHNESS = 0.88;
	SPECULAR = 0.25;
}
