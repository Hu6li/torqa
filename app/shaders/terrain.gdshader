shader_type spatial;
// Terrain tinted by its land cover (vertex colour from OpenStreetMap; alpha marks water), with
// rock on steep slopes and snow up high.

uniform vec3 rock_color : source_color = vec3(0.45, 0.43, 0.40);
uniform vec3 snow_color : source_color = vec3(0.93, 0.95, 0.97);
uniform float snow_line = 2400.0;
// Aerial photo of this chunk (north up) and the world rectangle it covers: x and z of the
// north-west corner, then width and depth in metres.
uniform sampler2D photo : source_color, filter_linear_mipmap_anisotropic, repeat_disable;
uniform bool has_photo = false;
uniform vec4 photo_rect = vec4(0.0, 0.0, 1.0, 1.0);

#include "res://shaders/aerial.gdshaderinc"

varying vec3 world_position;
varying vec3 world_normal;

float hash(vec2 p) {
	return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float value_noise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);
	vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(
		mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
		mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x),
		u.y
	);
}

void vertex() {
	world_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	world_normal = normalize((MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz);
}

void fragment() {
	// Large and small patches break up flat colours.
	float patches = value_noise(world_position.xz / 90.0) * 0.65 + value_noise(world_position.xz / 9.0) * 0.35;
	// Land-cover colours arrive as sRGB vertex colours; shading works in linear space.
	vec3 cover = pow(COLOR.rgb, vec3(2.2));
	vec3 color = cover * (0.85 + patches * 0.3);
	float steepness = 1.0 - world_normal.y;
	color = mix(color, rock_color, smoothstep(0.16, 0.30, steepness + patches * 0.05));
	float snow = smoothstep(snow_line - 150.0, snow_line + 150.0, world_position.y + patches * 120.0);
	color = mix(color, snow_color, snow * smoothstep(0.45, 0.25, steepness));

	if (has_photo) {
		vec2 uv = (world_position.xz - photo_rect.xy) / photo_rect.zw;
		color = aerial_albedo(texture(photo, uv).rgb);
		// Photo pixels are about half a metre wide, so up close they would look smeared; fine
		// noise gives the ground grain there.
		float near = 1.0 - smoothstep(15.0, 80.0, distance(world_position, CAMERA_POSITION_WORLD));
		float grain = value_noise(world_position.xz / 1.3) * 0.6 + value_noise(world_position.xz / 0.35) * 0.4;
		color *= 1.0 + (grain - 0.5) * 0.35 * near;
	}
	// Under lakes and rivers: the bed, mostly hidden by the water surface above it.
	float water = smoothstep(0.4, 0.6, COLOR.a);
	ALBEDO = mix(color, vec3(0.08, 0.1, 0.09), water);
	ROUGHNESS = 0.95;
	SPECULAR = 0.2;
}
