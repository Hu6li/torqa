class_name ProfileDialog
extends ConfirmationDialog
## Rider settings: the profile (fields mirror `TorqaApp.profile()`) and the rider's HUD layout
## (R51). Saving is left to the caller.

## The edited profile, shaped like `TorqaApp.profile()`, and HUD layout; `id` is empty for a new
## rider.
signal profile_confirmed(id: String, profile: Dictionary, hud_layout: PackedStringArray)

const UNITS: Array[String] = ["metric", "imperial"]
## Interface languages: locale code and name in that language; "" follows the system.
const LANGUAGES: Array[Array] = [["", "System language"], ["en", "English"], ["de", "Deutsch"]]

var _id: String = ""
var _name_edit: LineEdit = LineEdit.new()
var _rider_mass: SpinBox = _spin(30.0, 200.0, 0.5, " kg")
var _bike_mass: SpinBox = _spin(3.0, 40.0, 0.1, " kg")
var _ftp: SpinBox = _spin(50.0, 600.0, 1.0, " W")
var _max_heart_rate: SpinBox = _spin(100.0, 230.0, 1.0, " bpm")
var _units: OptionButton = OptionButton.new()
var _language: OptionButton = OptionButton.new()
var _avatar: OptionButton = OptionButton.new()
var _hud: HudEditor = HudEditor.new()


func _ready() -> void:
	theme = UiTheme.build()
	title = tr("Rider settings")
	ok_button_text = tr("Save")
	min_size = Vector2i(720, 460)
	var tabs: TabContainer = TabContainer.new()
	var grid: GridContainer = GridContainer.new()
	grid.name = tr("Profile")
	grid.columns = 2
	grid.add_theme_constant_override("h_separation", 24)
	grid.add_theme_constant_override("v_separation", 12)
	_name_edit.custom_minimum_size = Vector2(260, 0)
	# The fields grow with the dialog rather than staying fixed in the middle (R53).
	for field: Control in [
		_name_edit, _rider_mass, _bike_mass, _ftp, _max_heart_rate, _units, _language, _avatar
	]:
		field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_units.add_item(tr("Metric (km, kg)"))
	_units.add_item(tr("Imperial (mi, lb)"))
	# In the order of RiderAvatar.RIDERS.
	_avatar.add_item(tr("Female rider"))
	_avatar.add_item(tr("Male rider"))
	# Language names stay in their own language; only "System language" is translated.
	_language.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	for language: Array in LANGUAGES:
		var code: String = language[0]
		var language_name: String = language[1]
		_language.add_item(tr(language_name) if code.is_empty() else language_name)
	# i18n-begin
	for row: Array in [
		["Name", _name_edit],
		["Weight", _rider_mass],
		["Bike weight", _bike_mass],
		["FTP", _ftp],
		["Max heart rate", _max_heart_rate],
		["Units", _units],
		["Language", _language],
		["Rider on the bike", _avatar],
	]:
		# i18n-end
		var caption: Label = Label.new()
		caption.text = row[0]
		var field: Control = row[1]
		grid.add_child(caption)
		grid.add_child(field)
	tabs.add_child(grid)
	_hud.name = tr("HUD")
	tabs.add_child(_hud)
	add_child(tabs)
	_units.item_selected.connect(
		func(index: int) -> void: _hud.edit(_hud.layout(), UNITS[index] == "imperial")
	)
	confirmed.connect(_on_confirmed)


## Opens the dialog for `profile` (shaped like `TorqaApp.profile()`) with its `hud_layout`, or
## for a new rider if `profile` has no id.
func edit(profile: Dictionary, hud_layout: PackedStringArray) -> void:
	_id = profile.get("id", "")
	_name_edit.text = profile.get("name", "")
	_rider_mass.value = profile.get("rider_mass_kg", 75.0)
	_bike_mass.value = profile.get("bike_mass_kg", 8.0)
	_ftp.value = profile.get("ftp_w", 200.0)
	_max_heart_rate.value = profile.get("max_heart_rate_bpm", 185.0)
	_units.select(maxi(UNITS.find(profile.get("units", "metric")), 0))
	_language.select(0)
	for i: int in range(LANGUAGES.size()):
		if LANGUAGES[i][0] == profile.get("language", ""):
			_language.select(i)
	var avatar: String = profile.get("avatar", RiderAvatar.RIDERS[0])
	_avatar.select(maxi(RiderAvatar.RIDERS.find(avatar), 0))
	_hud.edit(hud_layout, UNITS[_units.selected] == "imperial")
	title = tr("New rider") if _id.is_empty() else tr("Rider settings")
	popup_centered(Vector2i(960, 600))
	_name_edit.grab_focus()


func _on_confirmed() -> void:
	var profile_name: String = _name_edit.text.strip_edges()
	(
		profile_confirmed
		. emit(
			_id,
			{
				"name": profile_name if not profile_name.is_empty() else tr("Rider"),
				"rider_mass_kg": _rider_mass.value,
				"bike_mass_kg": _bike_mass.value,
				"ftp_w": _ftp.value,
				"max_heart_rate_bpm": _max_heart_rate.value,
				"units": UNITS[_units.selected],
				"language": LANGUAGES[_language.selected][0],
				"avatar": RiderAvatar.RIDERS[_avatar.selected],
			},
			_hud.layout()
		)
	)


static func _spin(low: float, high: float, step: float, suffix: String) -> SpinBox:
	var spin: SpinBox = SpinBox.new()
	spin.min_value = low
	spin.max_value = high
	spin.step = step
	spin.suffix = suffix
	return spin
