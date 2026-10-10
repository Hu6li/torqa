class_name ProfileTab
extends VBoxContainer
## The riders (R22): one card per rider, the active one marked, each a line of key figures
## that unfolds to the rider's whole setup in three columns: settings, power zones and
## heart-rate zones (#191, #194). Riders can be deleted, all but the last. Switching riders
## also switches the interface language (R24).

## The active rider changed (figures, units, language or HUD).
signal profile_changed

var _torqa: TorqaApp
var _cards: VBoxContainer = VBoxContainer.new()
var _add_button: Button = Button.new()
var _dialog: ProfileDialog = ProfileDialog.new()
var _confirm_delete: ConfirmationDialog = ConfirmationDialog.new()
## The rider the confirmation asks about.
var _deleting: String = ""
## The riders unfolded, by id; kept across refreshes.
var _unfolded: Dictionary[String, bool] = {}


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	refresh()


## Lists the riders with the active one marked and their figures.
func refresh() -> void:
	var active: Dictionary = _torqa.profile()
	var language: String = active.get("language", "")
	apply_language(language)
	var active_id: String = active.get("id", "")
	_show_riders(_torqa.profiles(), active_id)


## Switches the interface to a rider's language; "" follows the system.
static func apply_language(code: String) -> void:
	var locale: String = code if not code.is_empty() else OS.get_locale_language()
	if TranslationServer.get_locale() != locale:
		TranslationServer.set_locale(locale)


func _init() -> void:
	add_theme_constant_override("separation", 16)
	var row: HBoxContainer = HBoxContainer.new()
	var heading: Label = Label.new()
	heading.text = tr("Riders")
	heading.add_theme_font_size_override("font_size", 22)
	heading.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.add_child(heading)
	_add_button.icon = UiIcons.texture("plus")
	_add_button.tooltip_text = tr("New rider…")
	_add_button.focus_mode = Control.FOCUS_NONE
	_add_button.pressed.connect(func() -> void: _dialog.edit({}, TorqaApp.hud_default_layout()))
	row.add_child(_add_button)
	add_child(row)
	_cards.add_theme_constant_override("separation", 12)
	add_child(_cards)
	add_child(_dialog)
	_dialog.profile_confirmed.connect(_on_profile_confirmed)
	_confirm_delete.title = tr("Delete rider?")
	_confirm_delete.ok_button_text = tr("Delete")
	UiTheme.danger_button(_confirm_delete.get_ok_button())
	# The text holds the rider's name, which is never translated.
	_confirm_delete.get_label().auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_confirm_delete.confirmed.connect(_delete)
	add_child(_confirm_delete)


## Builds a card per rider (dictionaries as `TorqaApp.profile()` gives them).
func _show_riders(riders: Array, active_id: String) -> void:
	for child: Node in _cards.get_children():
		_cards.remove_child(child)
		# Not free(): a card's own Use or chevron button is still emitting its pressed signal.
		child.queue_free()
	for rider: Dictionary in riders:
		var id: String = rider.get("id", "")
		_cards.add_child(_card(rider, id == active_id, riders.size() > 1))


## One rider: badge, name and key figures on a line, the way to use, edit or delete them,
## and the whole setup under it when unfolded.
func _card(rider: Dictionary, active: bool, deletable: bool) -> PanelContainer:
	var id: String = rider.get("id", "")
	var card: PanelContainer = PanelContainer.new()
	var box: StyleBoxFlat = UiTheme.panel()
	if active:
		box.border_color = Color(UiTheme.ACCENT, 0.8)
		box.set_border_width_all(2)
	card.add_theme_stylebox_override("panel", box)
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 14)
	card.add_child(rows)

	var line: HBoxContainer = HBoxContainer.new()
	line.add_theme_constant_override("separation", 12)
	var rider_name: String = rider.get("name", "")
	line.add_child(UiTheme.initial(rider_name, 44))
	var titles: VBoxContainer = VBoxContainer.new()
	titles.add_theme_constant_override("separation", 0)
	titles.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	titles.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	var name_label: Label = Label.new()
	name_label.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	name_label.text = rider_name
	name_label.add_theme_font_size_override("font_size", 20)
	titles.add_child(name_label)
	var figures: Label = Label.new()
	figures.text = _key_figures(rider)
	figures.add_theme_color_override("font_color", UiTheme.MUTED)
	titles.add_child(figures)
	line.add_child(titles)
	if active:
		var chip: PanelContainer = PanelContainer.new()
		chip.add_theme_stylebox_override("panel", UiTheme.button_chip())
		chip.size_flags_vertical = Control.SIZE_SHRINK_CENTER
		var chip_label: Label = Label.new()
		chip_label.text = tr("Active")
		chip.add_child(chip_label)
		line.add_child(chip)
	else:
		var use: Button = Button.new()
		use.text = tr("Use")
		use.tooltip_text = tr("Ride as this rider")
		use.focus_mode = Control.FOCUS_NONE
		use.size_flags_vertical = Control.SIZE_SHRINK_CENTER
		use.pressed.connect(_use.bind(id))
		line.add_child(use)
	var edit: Button = Button.new()
	edit.icon = UiIcons.texture("pencil")
	edit.tooltip_text = tr("Edit…")
	edit.focus_mode = Control.FOCUS_NONE
	edit.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	edit.pressed.connect(_edit.bind(id))
	line.add_child(edit)
	var delete: Button = Button.new()
	delete.icon = UiIcons.texture("bin")
	# There is always a rider to ride as.
	delete.disabled = not deletable
	delete.tooltip_text = (
		tr("Delete rider") if deletable else tr("The only rider cannot be deleted")
	)
	delete.focus_mode = Control.FOCUS_NONE
	delete.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	delete.pressed.connect(_ask_delete.bind(id, rider_name))
	UiTheme.danger_button(delete)
	line.add_child(delete)
	var unfolded: bool = _unfolded.get(id, false)
	var fold: Button = Button.new()
	fold.icon = UiIcons.texture("up" if unfolded else "down")
	fold.tooltip_text = tr("Fewer") if unfolded else tr("All settings")
	fold.focus_mode = Control.FOCUS_NONE
	fold.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	fold.pressed.connect(_toggle.bind(id))
	line.add_child(fold)
	rows.add_child(line)

	if unfolded:
		var columns: HBoxContainer = HBoxContainer.new()
		columns.add_theme_constant_override("separation", 32)
		var settings: GridContainer = GridContainer.new()
		settings.columns = 2
		settings.add_theme_constant_override("h_separation", 24)
		settings.add_theme_constant_override("v_separation", 8)
		settings.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		for row: Array in _setting_rows(rider):
			var caption: String = row[0]
			settings.add_child(UiTheme.caption(caption))
			var value: Label = Label.new()
			var text: String = row[1]
			value.text = tr(text)
			settings.add_child(value)
		columns.add_child(settings)
		_show_zones(columns, rider)
		rows.add_child(columns)
	return card


## The line under the name: FTP and W/kg, weight, maximum heart rate.
func _key_figures(rider: Dictionary) -> String:
	var imperial: bool = rider.get("units", "metric") == "imperial"
	var weight: float = rider.get("rider_mass_kg", 0.0)
	var ftp: float = rider.get("ftp_w", 0.0)
	var max_hr: float = rider.get("max_heart_rate_bpm", 0.0)
	var mass: String = "%.1f lb" % (weight * 2.20462) if imperial else "%.1f kg" % weight
	return (
		"%d W  ·  %.1f W/kg  ·  %s  ·  %d bpm"
		% [roundi(ftp), ftp / maxf(weight, 1.0), mass, roundi(max_hr)]
	)


## Everything the dialog has, as `[caption, value]` rows (#194); the HUD only as default or
## custom, its metrics would make the column far too long.
func _setting_rows(rider: Dictionary) -> Array[Array]:
	var imperial: bool = rider.get("units", "metric") == "imperial"
	var weight: float = rider.get("rider_mass_kg", 0.0)
	var bike: float = rider.get("bike_mass_kg", 0.0)
	var ftp: float = rider.get("ftp_w", 0.0)
	var max_hr: float = rider.get("max_heart_rate_bpm", 0.0)
	var mass: String = "%.1f lb" if imperial else "%.1f kg"
	var factor: float = 2.20462 if imperial else 1.0
	var language: String = tr("System language")
	var chosen: String = rider.get("language", "")
	for entry: Array in ProfileDialog.LANGUAGES:
		var code: String = entry[0]
		var language_name: String = entry[1]
		if code == chosen and not code.is_empty():
			language = language_name
	var single_cog: bool = rider.get("drivetrain", "cassette") == "single_cog"
	var id: String = rider.get("id", "")
	var hud: PackedStringArray = (
		_torqa.hud_layout_of(id) if _torqa != null else TorqaApp.hud_default_layout()
	)
	var custom_hud: bool = hud != TorqaApp.hud_default_layout()
	var difficulty: float = rider.get("default_difficulty_pct", 50.0)
	# i18n-begin
	var rows: Array[Array] = [
		["Weight", mass % (weight * factor)],
		["Bike weight", mass % (bike * factor)],
		["FTP", "%d W  ·  %.1f W/kg" % [roundi(ftp), ftp / maxf(weight, 1.0)]],
		["Max heart rate", "%d bpm" % roundi(max_hr)],
		["Units", "Imperial (mi, lb)" if imperial else "Metric (km, kg)"],
		["Rider on the bike", "Male rider" if rider.get("avatar") == "male" else "Female rider"],
		["Language", language],
		[
			"Drivetrain",
			"Single cog: virtual gears" if single_cog else "Cassette: shift on the bike",
		],
	]
	# i18n-end
	if single_cog:
		var chainring: int = rider.get("chainring", 50)
		var cog: int = rider.get("cog", 14)
		rows.append(["Chainring", "%d T" % chainring])
		rows.append(["Cog", "%d T" % cog])
	# i18n-begin
	rows.append(["HUD", "Custom" if custom_hud else "Default"])
	# i18n-end
	rows.append(["Trainer difficulty", "%d %%" % roundi(difficulty)])
	return rows


## The rider's zones (#173) as a column each in `columns`, every zone with its colour, name
## and range: power zones 1–7 from the bounds as shares of FTP, heart-rate zones 1–5 from the
## maximum heart rate.
func _show_zones(columns: HBoxContainer, rider: Dictionary) -> void:
	var ftp: float = rider.get("ftp_w", 200.0)
	var max_hr: float = rider.get("max_heart_rate_bpm", 185.0)
	var power: PackedFloat64Array = rider.get("power_zones_pct", ZonesEditor.DEFAULT_POWER)
	var heart: PackedFloat64Array = rider.get("heart_rate_zones_pct", ZonesEditor.DEFAULT_HEART)
	if power.size() != ZonesEditor.DEFAULT_POWER.size():
		power = ZonesEditor.DEFAULT_POWER
	if heart.size() != ZonesEditor.DEFAULT_HEART.size():
		heart = ZonesEditor.DEFAULT_HEART
	columns.add_child(_zone_column(tr("Power zones"), UiTheme.POWER_ZONES, power, 0.0, ftp, "W"))
	columns.add_child(
		_zone_column(
			tr("Heart-rate zones"),
			UiTheme.HEART_RATE_ZONES,
			heart,
			ZonesEditor.HEART_FLOOR_PCT,
			max_hr,
			"bpm"
		)
	)


## A caption over a row per zone: `bounds` are the tops of all but the last zone in percent
## of `base`; the first zone starts at `floor_pct`, the last one is open at the top.
func _zone_column(
	caption: String,
	zones: Array[Array],
	bounds: PackedFloat64Array,
	floor_pct: float,
	base: float,
	unit: String
) -> VBoxContainer:
	var column: VBoxContainer = VBoxContainer.new()
	column.add_theme_constant_override("separation", 6)
	column.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	column.add_child(UiTheme.caption(caption))
	var low_pct: float = floor_pct
	for i: int in range(zones.size()):
		var zone_name: String = zones[i][0]
		var color: Color = zones[i][1]
		var row: HBoxContainer = HBoxContainer.new()
		row.add_theme_constant_override("separation", 10)
		var chip: ColorRect = ColorRect.new()
		chip.color = color
		chip.custom_minimum_size = Vector2(10, 10)
		chip.size_flags_vertical = Control.SIZE_SHRINK_CENTER
		row.add_child(chip)
		var label: Label = Label.new()
		label.text = "Z%d %s" % [i + 1, tr(zone_name)]
		label.custom_minimum_size = Vector2(150, 0)
		row.add_child(label)
		var range_label: Label = Label.new()
		range_label.add_theme_color_override("font_color", UiTheme.MUTED)
		if i < bounds.size():
			var high_pct: float = bounds[i]
			range_label.text = (
				"%d–%d %s  ·  %d–%d %%"
				% [
					roundi(base * low_pct / 100.0),
					roundi(base * high_pct / 100.0),
					unit,
					roundi(low_pct),
					roundi(high_pct),
				]
			)
			low_pct = high_pct
		else:
			range_label.text = (
				"%d+ %s  ·  %d+ %%" % [roundi(base * low_pct / 100.0), unit, roundi(low_pct)]
			)
		row.add_child(range_label)
		column.add_child(row)
	return column


func _toggle(id: String) -> void:
	_unfolded[id] = not _unfolded.get(id, false)
	if _torqa != null:
		refresh()


func _use(id: String) -> void:
	_torqa.select_profile(id)
	refresh()
	profile_changed.emit()


## Editing a rider makes them the active one: saving does anyway, and the HUD layout edited
## is the active rider's.
func _edit(id: String) -> void:
	if id != _torqa.profile().get("id", ""):
		_torqa.select_profile(id)
		refresh()
		profile_changed.emit()
	_dialog.edit(_torqa.profile(), _torqa.hud_layout())


func _ask_delete(id: String, rider_name: String) -> void:
	_deleting = id
	_confirm_delete.dialog_text = tr("“%s” and all their rides are deleted.") % rider_name
	_confirm_delete.popup_centered()


func _delete() -> void:
	var was_active: bool = _deleting == _torqa.profile().get("id", "")
	if not _torqa.delete_profile(_deleting):
		return
	_unfolded.erase(_deleting)
	refresh()
	if was_active:
		profile_changed.emit()


func _on_profile_confirmed(id: String, profile: Dictionary, hud_layout: PackedStringArray) -> void:
	# Saving makes the rider active, so the layout goes to the right rider.
	if not _torqa.save_profile(id, profile).is_empty():
		_torqa.set_hud_layout(hud_layout)
	refresh()
	profile_changed.emit()
