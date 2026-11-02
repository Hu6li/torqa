class_name UiIcons
extends RefCounted
## Line icons for the interface's buttons, drawn from small SVGs in a theme colour: crisp at
## any size, and no image files to keep. Each is the paths of a 24 × 24 drawing.

const PATHS: Dictionary[String, String] = {
	"cog":
	(
		'<circle cx="12" cy="12" r="3"/><path d="M12 2v2.5M12 19.5V22M2 12h2.5M19.5 12H22'
		+ 'M4.9 4.9l1.8 1.8M17.3 17.3l1.8 1.8M4.9 19.1l1.8-1.8M17.3 6.7l1.8-1.8"/>'
		+ '<circle cx="12" cy="12" r="7"/>'
	),
	"overlay":
	(
		'<rect x="3" y="4" width="18" height="14" rx="2"/>'
		+ '<rect x="12" y="9" width="7" height="7" rx="1.5" fill="COLOR" stroke="none"/>'
	),
	"flag": '<path d="M5 21V4"/><path d="M5 4h11l-2 4 2 4H5"/>',
	"fullscreen": '<path d="M9 3H3v6M15 3h6v6M21 15v6h-6M9 21H3v-6"/>',
	"windowed": '<path d="M3 9h6V3M15 3v6h6M21 15h-6v6M9 21v-6H3"/>',
	# The chevron is cut out of the disc, so whatever lies behind shows through in any colour.
	"back":
	(
		'<defs><mask id="cut"><rect width="24" height="24" fill="white" stroke="none"/>'
		+ '<path d="M13.5 7.5L9 12l4.5 4.5" stroke="black" stroke-width="2.4"/></mask></defs>'
		+ '<circle cx="12" cy="12" r="9.5" fill="COLOR" stroke="none" mask="url(#cut)"/>'
	),
	"fold": '<path d="M15 6l-6 6 6 6"/>',
	"unfold": '<path d="M9 6l6 6-6 6"/>',
	"down": '<path d="M6 9l6 6 6-6"/>',
	"up": '<path d="M6 15l6-6 6 6"/>',
	"pause": '<path d="M8 5v14"/><path d="M16 5v14"/>',
	"play": '<path d="M7 4l12 8-12 8z" fill="COLOR" stroke="none"/>',
	"pencil": '<path d="M4 20l4-1 10-10-3-3L5 16z"/><path d="M13 7l3 3"/>',
	"bin": '<path d="M4 7h16"/><path d="M9 7V4h6v3"/><path d="M6 7l1 13h10l1-13"/>',
	"download": '<path d="M12 4v11"/><path d="M7 10l5 5 5-5"/><path d="M5 20h14"/>',
	"upload": '<path d="M12 15V4"/><path d="M7 9l5-5 5 5"/><path d="M5 20h14"/>',
	"plus": '<path d="M12 5v14"/><path d="M5 12h14"/>',
	"cross": '<path d="M6 6l12 12"/><path d="M18 6L6 18"/>',
	"search": '<circle cx="11" cy="11" r="6"/><path d="M20 20l-4.5-4.5"/>',
	"info":
	(
		'<circle cx="12" cy="12" r="9"/><path d="M12 11v5"/>'
		+ '<circle cx="12" cy="8" r="1" fill="COLOR" stroke="none"/>'
	),
	"qr":
	(
		'<rect x="4" y="4" width="6" height="6" rx="1"/>'
		+ '<rect x="14" y="4" width="6" height="6" rx="1"/>'
		+ '<rect x="4" y="14" width="6" height="6" rx="1"/>'
		+ '<path d="M14 14h2v2M20 14v6h-6M17 17v3"/>'
	),
	"group":
	(
		'<circle cx="9" cy="8" r="3.2"/><path d="M3 20c0-3.4 2.7-6 6-6s6 2.6 6 6"/>'
		+ '<path d="M15.5 4.9a3.2 3.2 0 0 1 0 6.2"/><path d="M17.5 14.3c2 .8 3.5 2.9 3.5 5.7"/>'
	),
	"warning":
	(
		'<path d="M12 3.5L2.5 20h19z"/><path d="M12 10v4.5"/>'
		+ '<circle cx="12" cy="17.2" r="1" fill="COLOR" stroke="none"/>'
	),
	"ascending": '<path d="M12 19V5"/><path d="M6 11l6-6 6 6"/>',
	"descending": '<path d="M12 5v14"/><path d="M6 13l6 6 6-6"/>',
	"speaking":
	(
		'<path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z" fill="COLOR"/>'
		+ '<path d="M15.5 9a4 4 0 0 1 0 6"/><path d="M18.5 6.5a7.5 7.5 0 0 1 0 11"/>'
	),
}
static var _cache: Dictionary[String, ImageTexture] = {}


## The icon `name` at `size` pixels, in `color`.
static func texture(name: String, size: int = 20, color: Color = UiTheme.TEXT) -> ImageTexture:
	var key: String = "%s/%d/%s" % [name, size, color.to_html()]
	if _cache.has(key):
		return _cache[key]
	var hex: String = "#" + color.to_html(false)
	var svg: String = (
		(
			'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" '
			+ 'fill="none" stroke="COLOR" stroke-width="1.8" stroke-linecap="round" '
			+ 'stroke-linejoin="round" opacity="%.3f">%s</svg>'
		)
		% [color.a, PATHS[name]]
	)
	var image: Image = Image.new()
	image.load_svg_from_string(svg.replace("COLOR", hex), size / 24.0)
	var icon: ImageTexture = ImageTexture.create_from_image(image)
	_cache[key] = icon
	return icon
