shader_type spatial;
// Terrain coloured by slope and altitude: meadow, rock on steep slopes, snow up high.

uniform vec3 grass_color : source_color = vec3(0.30, 0.46, 0.20);
uniform vec3 dry_grass_color : source_color = vec3(0.46, 0.50, 0.27);
uniform vec3 rock_color : source_color = vec3(0.45, 0.43, 0.40);
uniform vec3 snow_color : source_color = vec3(0.93, 0.95, 0.97);
uniform float snow_line = 2400.0;

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
	// Large and small patches break up the uniform green.
	float patches = value_noise(world_position.xz / 90.0) * 0.65 + value_noise(world_position.xz / 9.0) * 0.35;
	vec3 color = mix(grass_color, dry_grass_color, smoothstep(0.35, 0.85, patches));
	float steepness = 1.0 - world_normal.y;
	color = mix(color, rock_color, smoothstep(0.16, 0.30, steepness + patches * 0.05));
	float snow = smoothstep(snow_line - 150.0, snow_line + 150.0, world_position.y + patches * 120.0);
	color = mix(color, snow_color, snow * smoothstep(0.45, 0.25, steepness));
	ALBEDO = color;
	ROUGHNESS = 0.95;
	SPECULAR = 0.2;
}
