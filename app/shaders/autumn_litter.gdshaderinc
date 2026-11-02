#include "res://shaders/autumn.gdshaderinc"

uniform sampler2D autumn_forest_mask : filter_linear, repeat_disable;
uniform vec2 autumn_forest_origin = vec2(0.0);
uniform vec2 autumn_forest_size = vec2(0.0);

float autumn_wood(vec2 place) {
	vec2 cell = (place - autumn_forest_origin) / autumn_forest_size;
	if (any(lessThan(cell, vec2(0.0))) || any(greaterThan(cell, vec2(1.0)))) { return 0.0; }
	return texture(autumn_forest_mask, cell).r;
}

void autumn_litter(vec3 position, float edge, float marking, float wet, inout vec3 albedo, inout float roughness) {
	if (autumn_strength == 0.0 || autumn_forest_size.x == 0.0) { return; }
	float wood = max(autumn_wood(position.xz), max(max(autumn_wood(position.xz + vec2(6, 0)),
		autumn_wood(position.xz - vec2(6, 0))), max(autumn_wood(position.xz + vec2(0, 6)),
		autumn_wood(position.xz - vec2(0, 6)))) * 0.8);
	float amount = wood * autumn_amount(position.y);
	if (amount < 0.01) { return; }
	vec2 grid = position.xz / 0.14;
	vec2 cell = floor(grid);
	float seed = ground_hash(cell);
	vec2 jitter = vec2(ground_hash(cell + 17.0), ground_hash(cell - 23.0));
	vec2 point = (fract(grid) - mix(vec2(0.26), vec2(0.74), jitter)) * 0.14;
	float angle = seed * 6.2831853;
	point = mat2(vec2(cos(angle), -sin(angle)), vec2(sin(angle), cos(angle))) * point;
	float length = mix(0.022, 0.042, ground_hash(cell + 9.0));
	float along = point.x / length;
	float width = length * 0.44 * pow(max(0.0, 1.0 - abs(along)), 0.65);
	float soft = max(fwidth(position.x) + fwidth(position.z), 0.001);
	float leaf = (1.0 - smoothstep(width - soft, width + soft, abs(point.y)))
		* (1.0 - smoothstep(1.0 - soft / length, 1.0 + soft / length, abs(along)));
	float drift = ground_noise(position.xz / 2.7);
	float density = mix(0.12, 0.78, smoothstep(0.20, 0.90, edge));
	density *= mix(0.35, 1.0, drift) * mix(0.55, 1.0, autumn_collection(position.xz));
	// Subpixel leaves become their mean coverage, keeping distant roads free of shimmer.
	leaf = mix(leaf * step(seed, density), density * 0.05, smoothstep(0.012, 0.055, soft));
	leaf *= amount * (1.0 - marking * 0.75);
	vec3 colour = autumn_colors[int(ground_hash(cell + 37.0) * 3.999)].rgb;
	float rib = 1.0 - smoothstep(0.0, 0.0018, abs(point.y));
	colour *= mix(0.75, 1.12, seed) * (1.0 - rib * 0.24) * (1.0 - wet * 0.25);
	albedo = mix(albedo, colour, leaf);
	roughness = mix(roughness, mix(0.96, 0.55, wet), leaf);
}
