// Aerial photos already contain sunlight and atmospheric haze; the scene lights them again, so
// they are darkened and their colours restored before use as albedo.
vec3 aerial_albedo(vec3 raw) {
	vec3 color = pow(raw, vec3(1.15)) * 0.8;
	float luma = dot(color, vec3(0.2126, 0.7152, 0.0722));
	return max(mix(vec3(luma), color, 1.2), vec3(0.0));
}
