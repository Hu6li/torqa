shader_type spatial;
// Low-poly clouds (ADR 0011, art/clouds): faceted puffs, bright where they face the sun, shaded
// underneath and away from it, greyer in bad weather. At their distance the world's haze would
// hide them, so they skip it and fade into the horizon's colour themselves.
render_mode unshaded, fog_disabled;

uniform vec3 cloud_color : source_color = vec3(0.984, 0.969, 0.933);
uniform vec3 shade_color : source_color = vec3(0.78, 0.827, 0.824);
uniform vec3 horizon_color : source_color = vec3(0.839, 0.925, 0.894);
// The sun's colour tints the lit sides (warm in the morning and evening).
uniform vec3 sun_color : source_color = vec3(1.0, 0.945, 0.863);
// Unit vector towards the sun, in world space.
uniform vec3 sun_direction = vec3(0.0, 1.0, 0.0);
// 0 fair, 1 overcast: darker, greyer clouds.
uniform float darkness : hint_range(0.0, 1.0) = 0.0;
// Clouds fade into the horizon between these distances (metres).
uniform float fade_start = 7000.0;
uniform float fade_end = 12000.0;

void fragment() {
	vec3 facet = normalize(cross(dFdy(VERTEX), dFdx(VERTEX)));
	vec3 world_facet = (INV_VIEW_MATRIX * vec4(facet, 0.0)).xyz;
	float sun = max(dot(world_facet, sun_direction), 0.0);
	// Undersides are shaded whatever the sun does.
	float top = smoothstep(-0.6, 0.4, world_facet.y);
	vec3 lit = mix(cloud_color * mix(vec3(1.0), sun_color, 0.6), shade_color, darkness * 0.5);
	vec3 shade = shade_color * mix(1.0, 0.72, darkness);
	vec3 color = mix(shade, lit, top * (0.45 + 0.55 * sun));
	float far = smoothstep(fade_start, fade_end, length(VERTEX));
	ALBEDO = mix(color, horizon_color, far);
}
