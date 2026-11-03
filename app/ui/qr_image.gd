class_name QrImage
extends RefCounted
## A QR code as the core gives it (`{size, modules}`, e.g. `OrqwaApp.qr_code()`) drawn as an
## image a phone can read off the screen: dark on light, as readers need, with its quiet zone.

## Pixels per module: large enough to scan off a screen at arm's length.
const MODULE_PIXELS: int = 6
## Light modules around the code, as QR readers need.
const QUIET_ZONE: int = 4


## The code with its quiet zone, a pixel per module scaled up without smoothing, so it stays
## sharp at any size; `null` for an empty code.
static func texture(code: Dictionary, module_pixels: int = MODULE_PIXELS) -> ImageTexture:
	if code.is_empty():
		return null
	var size: int = code["size"]
	var data: PackedByteArray = code["modules"]
	var modules: Image = Image.create_from_data(size, size, false, Image.FORMAT_L8, data)
	var side: int = size + 2 * QUIET_ZONE
	var image: Image = Image.create_empty(side, side, false, Image.FORMAT_L8)
	image.fill(Color.WHITE)
	image.blit_rect(modules, Rect2i(0, 0, size, size), Vector2i(QUIET_ZONE, QUIET_ZONE))
	image.resize(side * module_pixels, side * module_pixels, Image.INTERPOLATE_NEAREST)
	return ImageTexture.create_from_image(image)
