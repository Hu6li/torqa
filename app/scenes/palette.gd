class_name Palette
extends RefCounted
## The colour palette of the stylized look (ADR 0011), read from the same file as the Rust core
## (app/assets/palette.json), so a colour changes in one place. Colours are sRGB, as Godot's are.

const SOURCE: String = "res://assets/palette.json"

static var _sections: Dictionary = {}


## The colour `section.name`, e.g. "sky.top".
static func color(path: String) -> Color:
	var value: String = _entry(path)
	return Color.html(value)


## The colours of the list `section.name`, e.g. "plants.flowers".
static func colors(path: String) -> PackedColorArray:
	var list: Array = _entry(path)
	var result: PackedColorArray = PackedColorArray()
	for value: String in list:
		result.append(Color.html(value))
	return result


static func _entry(path: String) -> Variant:
	if _sections.is_empty():
		var json: JSON = load(SOURCE)
		_sections = json.data
	var parts: PackedStringArray = path.split(".")
	var section: Dictionary = _sections[parts[0]]
	return section[parts[1]]
