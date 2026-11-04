class_name ProfileDialog
extends ConfirmationDialog
## Rider settings: the profile (fields mirror `OrqwaApp.profile()`) and the rider's ride and
## workout HUD layouts (R51, R207). Saving is left to the caller.

## The edited profile, shaped like `OrqwaApp.profile()`, and both HUD layouts; `id` is empty
## for a new rider.
signal profile_confirmed(
	id: String, profile: Dictionary, ride_hud: PackedStringArray, workout_hud: PackedStringArray
)

var _id: String = ""
var _name_edit: LineEdit = LineEdit.new()
var _form: ProfileForm = ProfileForm.new()
var _hud: HudEditor = HudEditor.new()
var _hud_page: VBoxContainer = VBoxContainer.new()
var _hud_choice: OptionButton = OptionButton.new()
var _hud_layouts: Array[PackedStringArray] = []
var _hud_index: int = 0
var _badge_slot: HBoxContainer = HBoxContainer.new()
var _zones: ZonesEditor = ZonesEditor.new()
## The units the HUD editor shows its figures in, to follow a change of them.
var _imperial: bool = false


func _ready() -> void:
	theme = UiTheme.build()
	title = tr("Rider settings")
	ok_button_text = tr("Save")
	min_size = Vector2i(720, 460)
	var tabs: TabContainer = TabContainer.new()
	# The rider as a heading, an initial in a badge beside the name, then the figures in a
	# card (#191).
	var page: VBoxContainer = VBoxContainer.new()
	page.add_theme_constant_override("separation", 16)
	var header: HBoxContainer = HBoxContainer.new()
	header.add_theme_constant_override("separation", 12)
	_badge_slot.add_child(UiTheme.initial("", 48))
	header.add_child(_badge_slot)
	_name_edit.placeholder_text = tr("Name")
	_name_edit.add_theme_font_size_override("font_size", 22)
	_name_edit.custom_minimum_size = Vector2(260, 0)
	_name_edit.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_name_edit.text_changed.connect(_show_initial)
	header.add_child(_name_edit)
	page.add_child(header)
	var card: PanelContainer = PanelContainer.new()
	card.add_child(_form)
	page.add_child(card)
	var profile_page: ScrollContainer = UiTheme.scrolling(page)
	profile_page.name = tr("Profile")
	tabs.add_child(profile_page)
	var zones_page: ScrollContainer = UiTheme.scrolling(_zones)
	zones_page.name = tr("Zones")
	tabs.add_child(zones_page)
	_hud_page.name = tr("HUD")
	_hud_choice.add_item(tr("Ride HUD"))
	_hud_choice.add_item(tr("Workout HUD"))
	_hud_choice.item_selected.connect(_on_hud_selected)
	_hud_page.add_child(_hud_choice)
	_hud_page.add_child(_hud)
	tabs.add_child(_hud_page)
	add_child(tabs)
	_form.changed.connect(_on_form_changed)
	confirmed.connect(_on_confirmed)


## Opens the dialog for `profile` (shaped like `OrqwaApp.profile()`) with both HUD layouts, or
## for a new rider if `profile` has no id.
func edit(
	profile: Dictionary,
	ride_hud: PackedStringArray,
	workout_hud: PackedStringArray = PackedStringArray()
) -> void:
	_id = profile.get("id", "")
	_name_edit.text = profile.get("name", "")
	_show_initial(_name_edit.text)
	_form.edit(profile)
	var power_zones: PackedFloat64Array = profile.get("power_zones_pct", ZonesEditor.DEFAULT_POWER)
	var heart_zones: PackedFloat64Array = profile.get(
		"heart_rate_zones_pct", ZonesEditor.DEFAULT_HEART
	)
	_zones.edit(power_zones, heart_zones, _form.ftp(), _form.max_heart_rate())
	_imperial = _form.imperial()
	_hud_layouts = [ride_hud.duplicate(), workout_hud.duplicate()]
	if workout_hud.is_empty():
		_hud_layouts[1] = ride_hud.duplicate()
	_hud_index = 0
	_hud_choice.select(0)
	_hud.edit(ride_hud, _imperial)
	title = tr("New rider") if _id.is_empty() else tr("Rider settings")
	UiScale.popup(self, Vector2i(960, 780))
	_name_edit.grab_focus()


## FTP or maximum heart rate moved the zones' watts and beats; other units, the HUD's figures.
func _on_form_changed() -> void:
	_zones.set_bases(_form.ftp(), _form.max_heart_rate())
	if _form.imperial() != _imperial:
		_imperial = _form.imperial()
		_hud.edit(_hud.layout(), _imperial)


func _on_confirmed() -> void:
	var profile_name: String = _name_edit.text.strip_edges()
	var profile: Dictionary = _form.profile()
	profile["name"] = profile_name if not profile_name.is_empty() else tr("Rider")
	profile["power_zones_pct"] = _zones.power_pct()
	profile["heart_rate_zones_pct"] = _zones.heart_pct()
	_hud_layouts[_hud_index] = _hud.layout()
	profile_confirmed.emit(_id, profile, _hud_layouts[0], _hud_layouts[1])


func _on_hud_selected(index: int) -> void:
	_hud_layouts[_hud_index] = _hud.layout()
	_hud_index = index
	_hud.edit(_hud_layouts[index], _imperial)


func _show_initial(rider_name: String) -> void:
	for child: Node in _badge_slot.get_children():
		child.queue_free()
	_badge_slot.add_child(UiTheme.initial(rider_name, 48))
