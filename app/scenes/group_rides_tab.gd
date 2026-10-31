class_name GroupRidesTab
extends VBoxContainer
# gdlint: disable=max-line-length
## Rider-facing setup; membership, transfers and readiness belong to the Rust core.

signal devices_requested

var compact: bool = false
var _orqwa: OrqwaApp
var _setup: BoxContainer = BoxContainer.new()
var _body: BoxContainer = BoxContainer.new()
var _sidebar: VBoxContainer = VBoxContainer.new()
var _activity_panel: PanelContainer = PanelContainer.new()
var _picker: GroupActivityPicker = GroupActivityPicker.new()
var _stage: Label = Label.new()
var _drafting: CheckBox = CheckBox.new()
var _together: CheckBox = CheckBox.new()
var _rule_summary: Label = Label.new()
var _connecting: VBoxContainer = VBoxContainer.new()
var _request: Button
var _invite: VBoxContainer = VBoxContainer.new()
var _notice: Label = Label.new()
var _roster: VBoxContainer = VBoxContainer.new()
var _next: Label = Label.new()
var _ready_button: Button = Button.new()
var _start: Button = Button.new()
var _enter: Button = Button.new()
var _devices: Button
var _retry: Button
var _voice_controls: GroupVoiceControls = GroupVoiceControls.new()
var _voice_panel: VBoxContainer = VBoxContainer.new()
var _invitation_panel: PanelContainer
var _workout_panel: PanelContainer
var _workout_status: Label = Label.new()
var _workout_pause: Button
var _workout_next: Button
var _organizer_panel: PanelContainer
var _organizer_status: Label = Label.new()
var _organizer_change: Button
var _organizer_accept: Button
var _organizer_decline: Button
var _organizer_cancel: Button
var _organizer_picker: AcceptDialog = AcceptDialog.new()
var _organizer_choices: VBoxContainer = VBoxContainer.new()
var _progress: ProgressBar = ProgressBar.new()
var _bytes: Label = Label.new()
var _connection: GroupConnectionDialog = GroupConnectionDialog.new()
var _exchange: ConfirmationDialog = ConfirmationDialog.new()
var _paste: LineEdit = LineEdit.new()
var _exchange_help: Label = Label.new()
var _exchange_error: Label = Label.new()
var _exchange_kind: String = "join"
var _status: Dictionary = {}
var _roster_key: String = ""
var _activity_key: String = ""
var _elapsed: float = 0.0


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	_picker.bind(orqwa)
	_voice_controls.bind(orqwa)
	_connection.bind(orqwa)
	if is_node_ready():
		refresh()


func _ready() -> void:
	add_theme_constant_override("separation", 18)
	if not compact:
		_label(self, tr("Ride with friends"), 26)
	_stage.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_stage.add_theme_color_override("font_color", UiTheme.MUTED)
	add_child(_stage)
	_rule_summary.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_rule_summary.add_theme_color_override("font_color", UiTheme.MUTED)
	add_child(_rule_summary)
	_build_setup()
	_build_connecting()
	_body.add_theme_constant_override("separation", 24)
	_body.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	add_child(_body)
	_activity_panel.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_activity_panel.size_flags_stretch_ratio = 1.6
	_activity_panel.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	_activity_panel.add_child(_picker)
	_build_rules()
	_picker.chosen.connect(_select_activity)
	_body.add_child(_activity_panel)
	_sidebar.add_theme_constant_override("separation", 16)
	_sidebar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_sidebar.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	_body.add_child(_sidebar)
	_build_invitation()
	_build_roster()
	_build_organizer()
	_build_workout_controls()
	var audio: VBoxContainer = _card(_sidebar)
	var voice: Button = _button(
		audio, tr("Voice chat"), func() -> void: _voice_panel.visible = not _voice_panel.visible
	)
	voice.alignment = HORIZONTAL_ALIGNMENT_LEFT
	voice.icon = UiIcons.texture("down")
	_voice_panel.add_child(_voice_controls)
	_voice_panel.hide()
	audio.add_child(_voice_panel)
	_button(_sidebar, tr("Leave group"), func() -> void: _orqwa.group_leave())
	_build_dialogs()
	resized.connect(_reflow)
	visibility_changed.connect(refresh)
	_reflow()
	refresh()


func _build_rules() -> void:
	var row: HFlowContainer = HFlowContainer.new()
	row.add_theme_constant_override("h_separation", 24)
	_drafting.text = tr("Drafting")
	_drafting.tooltip_text = tr("Save effort when riding behind friends.")
	_together.text = tr("Keep together")
	_together.tooltip_text = tr("Help everyone stay with the organizer while pedaling.")
	for option: CheckBox in [_drafting, _together]:
		option.button_pressed = true
		option.toggled.connect(_change_rules)
		row.add_child(option)
	_picker.options.add_child(row)
	_muted(_picker.options, tr("Assisted rides count for training, but do not set speed records."))


func _change_rules(_on: bool) -> void:
	_orqwa.group_assistance(_drafting.button_pressed, _together.button_pressed)


func _build_setup() -> void:
	_setup.add_theme_constant_override("separation", 24)
	add_child(_setup)
	var host: VBoxContainer = _card(_setup)
	_label(host, tr("Start a group ride"), 22)
	_muted(host, tr("Choose the ride and invite your friends. You decide when everyone starts."))
	var create: Button = _button(
		host, tr("Create a ride"), func() -> void: _connection.ask_create()
	)
	create.custom_minimum_size.y = 48
	create.add_theme_stylebox_override("normal", UiTheme.accent_button())
	var join: VBoxContainer = _card(_setup)
	_label(join, tr("Joining friends?"), 22)
	_muted(join, tr("Ask the organizer for an invitation, then paste it here to join their ride."))
	var join_button: Button = _button(
		join, tr("Join a ride…"), func() -> void: _show_exchange("join")
	)
	join_button.custom_minimum_size.y = 48


func _build_connecting() -> void:
	add_child(_connecting)
	var card: VBoxContainer = _card(_connecting)
	_label(card, tr("Send your join request"), 22)
	_muted(
		card, tr("Copy your request and send it privately to the organizer. They'll let you in.")
	)
	_request = _button(card, tr("Copy join request"), _copy_request)
	_request.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_muted(
		card,
		tr(
			"Keep this page open. Requests expire after one minute; if needed, ask for a fresh invitation."
		)
	)
	_button(card, tr("Cancel joining"), func() -> void: _orqwa.group_leave())


func _build_invitation() -> void:
	var card: VBoxContainer = _card(_sidebar)
	_invitation_panel = card.get_parent() as PanelContainer
	card.add_child(_invite)
	_invite.add_theme_constant_override("separation", 10)
	_label(_invite, tr("Invite friends"), 20)
	_muted(
		_invite, tr("1. Send your invitation privately.\n2. Add each friend's join request here.")
	)
	var actions: HFlowContainer = HFlowContainer.new()
	_button(actions, tr("Copy invitation"), _copy_invite)
	_button(actions, tr("Add a friend…"), func() -> void: _show_exchange("admit"))
	_invite.add_child(actions)
	_notice.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_notice.add_theme_color_override("font_color", UiTheme.SUCCESS)
	_invite.add_child(_notice)


func _build_roster() -> void:
	var card: VBoxContainer = _card(_sidebar)
	_label(card, tr("Your group"), 20)
	_roster.add_theme_constant_override("separation", 12)
	card.add_child(_roster)
	card.add_child(HSeparator.new())
	_next.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_next.add_theme_color_override("font_color", UiTheme.MUTED)
	card.add_child(_next)
	_progress.show_percentage = false
	_progress.custom_minimum_size.y = 6
	card.add_child(_progress)
	_bytes.add_theme_color_override("font_color", UiTheme.MUTED)
	card.add_child(_bytes)
	_ready_button.text = tr("I'm ready")
	_ready_button.toggle_mode = true
	_ready_button.custom_minimum_size.y = 44
	_ready_button.toggled.connect(func(on: bool) -> void: _orqwa.group_ready(on))
	card.add_child(_ready_button)
	_start.text = tr("Start group ride")
	_start.custom_minimum_size.y = 52
	_start.add_theme_font_size_override("font_size", 20)
	_start.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_start.pressed.connect(func() -> void: _orqwa.group_start())
	card.add_child(_start)
	_enter.text = tr("Join ride now")
	_enter.custom_minimum_size.y = 52
	_enter.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_enter.pressed.connect(func() -> void: _orqwa.group_enter())
	card.add_child(_enter)
	_devices = _button(card, tr("Connect your trainer…"), func() -> void: devices_requested.emit())
	_retry = _button(card, tr("Retry download"), func() -> void: _orqwa.group_retry())


func _build_dialogs() -> void:
	for dialog: AcceptDialog in [_exchange]:
		dialog.theme = UiTheme.build()
		add_child(dialog)
	_exchange.dialog_hide_on_ok = false
	var fields: VBoxContainer = VBoxContainer.new()
	fields.add_theme_constant_override("separation", 14)
	_exchange_help.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_exchange_help.custom_minimum_size.x = 480
	fields.add_child(_exchange_help)
	_paste.max_length = 2048
	_paste.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_paste.text_changed.connect(
		func(value: String) -> void:
			_exchange.get_ok_button().disabled = value.strip_edges().is_empty()
	)
	_paste.text_submitted.connect(func(_value: String) -> void: _submit_exchange())
	fields.add_child(_paste)
	_exchange_error.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_exchange_error.custom_minimum_size.x = 480
	_exchange_error.add_theme_color_override("font_color", UiTheme.DANGER)
	fields.add_child(_exchange_error)
	_exchange.add_child(fields)
	_exchange.confirmed.connect(_submit_exchange)
	add_child(_connection)
	_connection.started.connect(_connection_started)
	_connection.join_cancelled.connect(func() -> void: _exchange.popup_centered(Vector2i(540, 0)))
	_organizer_picker.theme = UiTheme.build()
	_organizer_picker.title = tr("Choose a new organizer")
	_organizer_choices.add_theme_constant_override("separation", 12)
	_organizer_choices.custom_minimum_size.x = 380
	_organizer_picker.add_child(_organizer_choices)
	add_child(_organizer_picker)


func _build_organizer() -> void:
	var card: VBoxContainer = _card(_sidebar)
	_organizer_panel = card.get_parent() as PanelContainer
	_label(card, tr("Ride organizer"), 20)
	_organizer_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	card.add_child(_organizer_status)
	_organizer_change = _button(card, tr("Change organizer…"), _choose_organizer)
	_organizer_accept = _button(
		card, tr("Accept organizer duties"), func() -> void: _orqwa.group_handover_answer(true)
	)
	_organizer_accept.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_organizer_decline = _button(
		card, tr("Decline"), func() -> void: _orqwa.group_handover_answer(false)
	)
	_organizer_cancel = _button(
		card, tr("Cancel organizer change"), func() -> void: _orqwa.group_handover_cancel()
	)


func _choose_organizer() -> void:
	for child: Node in _organizer_choices.get_children():
		_organizer_choices.remove_child(child)
		child.queue_free()
	_muted(
		_organizer_choices,
		tr("Choose a friend who is ready. They must accept before anything changes.")
	)
	for rider: Dictionary in _status.get("roster", []):
		if rider["me"] or not rider["ready"]:
			continue
		var id: int = rider["id"]
		var rider_name: String = rider["name"]
		var choice: Button = _button(
			_organizer_choices,
			rider_name,
			func() -> void:
				_orqwa.group_handover(id)
				_organizer_picker.hide()
		)
		choice.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_organizer_picker.popup_centered()


func _show_organizer() -> void:
	var phase: String = _status.get("phase", "idle")
	var handover: String = _status.get("handover", "idle")
	var host: bool = _status.get("host", false)
	var offer: bool = _status.get("handover_offer", false)
	_organizer_panel.visible = phase == "riding" and (host or handover != "idle")
	_organizer_change.visible = host and handover == "idle"
	_organizer_change.disabled = not _status.get("can_handover", false)
	_organizer_accept.visible = offer
	_organizer_decline.visible = offer
	_organizer_cancel.visible = _status.get("can_cancel_handover", false) and not offer
	_organizer_status.text = tr(
		"You choose who organizes the ride. Your recording continues when you hand over."
	)
	if handover == "offered":
		_organizer_status.text = (
			tr("Waiting for %s to accept organizer duties.") % _status.get("successor_name", "")
		)
	if offer:
		_organizer_status.text = tr(
			"Would you like to organize this ride? You will control the workout and welcome new riders. Accepting shares connection addresses with everyone in this group, using your connection settings."
		)
	if handover in ["preparing", "committing"]:
		_organizer_status.text = (
			tr(
				"Connecting everyone to %s. Keep riding; your recording continues. Voice reconnects with your microphone off."
			)
			% _status.get("successor_name", "")
		)


func _build_workout_controls() -> void:
	var card: VBoxContainer = _card(_sidebar)
	_workout_panel = card.get_parent() as PanelContainer
	_label(card, tr("Group workout"), 20)
	_workout_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	card.add_child(_workout_status)
	_muted(
		card,
		tr("Your intensity stays personal. Pause in the ride controls stops only your own ride.")
	)
	_workout_pause = _button(
		card,
		tr("Pause workout for everyone"),
		func() -> void:
			var paused: bool = _status.get("workout_paused", false)
			_orqwa.group_workout_pause(not paused)
	)
	_workout_next = _button(card, tr("Next step for everyone"), func() -> void: _orqwa.skip_step())


func _reflow() -> void:
	_setup.vertical = size.x < 760
	_body.vertical = size.x < 760 or compact


func _card(parent: Node) -> VBoxContainer:
	var panel: PanelContainer = PanelContainer.new()
	panel.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var content: VBoxContainer = VBoxContainer.new()
	content.add_theme_constant_override("separation", 14)
	panel.add_child(content)
	parent.add_child(panel)
	return content


func _label(parent: Node, text: String, font_size: int = 15) -> Label:
	var label: Label = Label.new()
	label.text = text
	label.add_theme_font_size_override("font_size", font_size)
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	parent.add_child(label)
	return label


func _muted(parent: Node, text: String) -> void:
	_label(parent, text).add_theme_color_override("font_color", UiTheme.MUTED)


func _button(parent: Node, text: String, action: Callable) -> Button:
	var button: Button = Button.new()
	button.text = text
	button.pressed.connect(action)
	parent.add_child(button)
	return button


func refresh() -> void:
	if _orqwa == null or not is_node_ready():
		return
	_picker.refresh()
	_activity_key = ""
	_update()


func _process(delta: float) -> void:
	_elapsed += delta
	if _elapsed >= 0.2 and is_visible_in_tree() and _orqwa != null:
		_elapsed = 0.0
		_update()


func _update() -> void:
	_show_status(_orqwa.group_status())


func _show_status(status: Dictionary) -> void:
	_status = status
	var phase: String = _status.get("phase", "idle")
	var host: bool = _status.get("host", false)
	var prepared: bool = _status.get("prepared", false)
	var late: bool = _status.get("late", false)
	var entering: bool = _status.get("entering", false)
	var ready: bool = _status.get("ready", false)
	var name: String = _status.get("name", "")
	var countdown: float = _status.get("countdown_s", 0.0)
	var can_retry: bool = _status.get("can_retry", false)
	var underway: bool = phase in ["countdown", "riding", "ended"]
	var drafting: bool = _status.get("drafting", true)
	var together: bool = _status.get("keep_together", true)
	_drafting.set_pressed_no_signal(drafting)
	_together.set_pressed_no_signal(together)
	_drafting.disabled = not host or phase not in ["waiting", "ready"] or late
	_together.disabled = _drafting.disabled
	_rule_summary.text = (
		(tr("Drafting on") if drafting else tr("Drafting off"))
		+ " · "
		+ (tr("Keep together on") if together else tr("Keep together off"))
	)
	_rule_summary.visible = (
		phase in ["countdown", "riding"]
		or (late and not name.is_empty())
		or (not host and not name.is_empty() and phase in ["downloading", "preparing"])
	)
	var captions: Dictionary = {
		"idle": tr("Choose who starts the ride. You'll be riding together in a few steps."),
		"connecting": tr("Creating your ride…") if host else tr("Connecting to your friends…"),
		"waiting":
		(
			tr("Invite your friends, then choose what to ride.")
			if host
			else tr("You're in! The organizer is choosing your ride.")
		),
		"downloading": tr("Getting your activity from the organizer…"),
		"preparing": tr("Preparing your ride…"),
		"ready": tr("Your ride is ready. Let's get everyone to the start."),
		"countdown": tr("Starting in %s seconds…") % ceili(countdown),
		"riding": tr("Group ride in progress"),
		"ended": tr("Group connection ended. Your local ride continues."),
		"failed": tr("Could not connect. Leave the group and ask for a fresh invitation."),
	}
	_stage.text = captions.get(phase, "")
	if late and not underway:
		_stage.text = tr("Your friends are already riding. Get ready and join when you like.")
	_setup.visible = phase == "idle" and not compact
	_connecting.visible = phase == "connecting" and not host and not compact
	var reply: String = _status.get("reply", "")
	_request.disabled = reply.is_empty()
	_body.visible = phase != "idle" and not _connecting.visible
	_invite.visible = host and phase not in ["ended", "failed"]
	_invitation_panel.visible = _invite.visible
	_notice.visible = not _notice.text.is_empty()
	var invite: String = _status.get("invite", "")
	for action: Node in _invite.find_children("*", "Button", true, false):
		(action as Button).disabled = invite.is_empty() or _status.get("handover", "idle") != "idle"
	_activity_panel.visible = not compact and (host or (not name.is_empty() and prepared))
	var course_path: String = _status.get("course_path", "")
	var workout_id: String = _status.get("workout_id", "")
	var activity_failed: bool = _status.get("activity_failed", false)
	var activity_key: String = (
		course_path
		+ workout_id
		+ str(host)
		+ str(underway)
		+ str(activity_failed)
		+ TranslationServer.get_locale()
	)
	if activity_key != _activity_key:
		_activity_key = activity_key
		_picker.refresh()
		_picker.show_selection(
			host and not underway and not late,
			"" if activity_failed else course_path,
			"" if activity_failed else workout_id
		)
	var received: float = _status.get("received", 0)
	var total: float = _status.get("total", 0)
	_progress.visible = phase == "downloading" and total > 0
	_bytes.visible = _progress.visible
	_progress.max_value = maxf(1, total)
	_progress.value = received
	_bytes.text = tr("%.1f / %.1f MiB") % [received / 1048576, total / 1048576]
	var trainer: bool = _orqwa.trainer_connected()
	_next.text = (
		tr("Choose an activity to get started.")
		if host
		else tr("Waiting for the organizer to choose an activity.")
	)
	if not name.is_empty():
		_next.text = tr("Getting your ride ready…")
	if prepared:
		_next.text = (
			tr("Connect your trainer, then tell the group you're ready.")
			if not trainer
			else tr("All set? Let your friends know you're ready.")
		)
	if ready:
		_next.text = (
			tr("You're ready. Waiting for the rest of the group.")
			if host
			else tr("You're ready. The organizer will start the ride.")
		)
	if _status.get("can_start", false):
		_next.text = tr("Everyone is ready. Start when you like!")
	if underway:
		_next.text = _stage.text
	elif can_retry:
		_next.text = tr("Download stopped. Try again when you're ready.")
		_stage.text = _next.text
	if late and not underway and prepared:
		_next.text = (
			tr("Connect your trainer to join your friends.")
			if not trainer
			else tr(
				"Join near the organizer at the current workout step. Your recording starts now."
			)
		)
	if entering:
		_next.text = tr("Joining your friends…")
	_enter.visible = late and not underway
	_enter.disabled = not trainer or not _status.get("can_enter", false)
	_ready_button.visible = not underway and not late
	_ready_button.disabled = not prepared or (not trainer and not ready)
	_ready_button.set_pressed_no_signal(ready)
	_ready_button.text = tr("Ready ✓ · click to undo") if ready else tr("I'm ready")
	_start.visible = host and not underway and not compact
	_start.disabled = not _status.get("can_start", false)
	_devices.visible = not compact and not underway and not trainer
	_retry.visible = can_retry
	_voice_controls.update_status(_status)
	_show_workout_controls()
	_show_organizer()
	var key: String = (
		str(_status.get("roster", []))
		+ str(_status.get("voice", false))
		+ str(_status.get("local_paused", false))
		+ phase
		+ TranslationServer.get_locale()
	)
	if key != _roster_key:
		_roster_key = key
		_show_roster(phase, host)


func _show_workout_controls() -> void:
	_workout_panel.visible = _status.get("workout_shared", false)
	var paused: bool = _status.get("workout_paused", false)
	_workout_status.text = (
		tr("Workout paused for the group. You can pedal freely.")
		if paused
		else tr("The organizer controls workout timing for everyone.")
	)
	if _status.get("workout_complete", false):
		_workout_status.text = tr("Group workout complete")
	_workout_pause.text = (
		tr("Resume workout for everyone") if paused else tr("Pause workout for everyone")
	)
	_workout_pause.visible = _status.get("host", false)
	_workout_next.visible = _workout_pause.visible
	_workout_pause.disabled = not _status.get("can_control_workout", false)
	_workout_next.disabled = _workout_pause.disabled


func _show_roster(phase: String, host: bool) -> void:
	for child: Node in _roster.get_children():
		_roster.remove_child(child)
		child.queue_free()
	for rider: Dictionary in _status.get("roster", []):
		var row: HBoxContainer = HBoxContainer.new()
		row.add_theme_constant_override("separation", 10)
		var rider_name: String = rider["name"]
		row.add_child(UiTheme.initial(rider_name, 36))
		var text: VBoxContainer = VBoxContainer.new()
		text.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		var label: Label = _label(text, rider_name, 17)
		label.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		var state: Label = _label(text, tr("Ready") if rider["ready"] else tr("Getting ready"), 13)
		if phase == "riding":
			state.text = tr("No signal") if rider["ready"] else tr("Getting ready")
			if rider["me"]:
				state.text = tr("Paused") if _status.get("local_paused", false) else tr("Riding")
		var live: bool = (
			rider["ready"] and (phase != "riding" or rider["me"] or rider.get("present", false))
		)
		state.add_theme_color_override("font_color", UiTheme.SUCCESS if live else UiTheme.MUTED)
		if rider["me"]:
			state.text += " · " + tr("You")
		if phase == "riding" and rider.get("present", false):
			var imperial: bool = _orqwa.profile().get("units", "metric") == "imperial"
			var gap: float = rider["gap_m"]
			state.text = (
				tr("%s ft") % roundi(gap / HudPanel.METERS_PER_FOOT)
				if imperial
				else tr("%s m") % roundi(gap)
			)
		if rider.get("organizer", false):
			state.text += " · " + tr("Organizer")
		row.add_child(text)
		if not rider["me"] and _status.get("voice", false):
			var speaker: int = rider["id"]
			var mute: Button = Button.new()
			mute.text = tr("Mute")
			mute.tooltip_text = tr("Mute this rider for me")
			mute.toggle_mode = true
			var muted: bool = rider.get("muted", false)
			mute.set_pressed_no_signal(muted)
			mute.toggled.connect(func(on: bool) -> void: _orqwa.group_voice_mute(speaker, on))
			row.add_child(mute)
		if host and not rider["me"] and phase not in ["countdown", "riding", "ended"]:
			var id: int = rider["id"]
			var remove: Button = _button(row, "", func() -> void: _orqwa.group_remove(id))
			remove.icon = UiIcons.texture("cross")
			remove.tooltip_text = tr("Remove from group")
		_roster.add_child(row)


func _select_activity(course: String, workout: String) -> void:
	_orqwa.group_select(course, workout)
	_activity_key = ""
	_update()


func _show_exchange(kind: String) -> void:
	_exchange_kind = kind
	_exchange.title = tr("Join a ride") if kind == "join" else tr("Add a friend")
	_exchange_help.text = (
		tr("Paste the invitation your organizer sent you.")
		if kind == "join"
		else tr("Paste the join request your friend sent back. Only add people you invited.")
	)
	_exchange.ok_button_text = tr("Continue") if kind == "join" else tr("Add friend")
	_paste.placeholder_text = (
		tr("Paste invitation here") if kind == "join" else tr("Paste join request here")
	)
	_paste.clear()
	_exchange_error.text = ""
	_exchange_error.hide()
	_exchange.get_ok_button().disabled = true
	_exchange.popup_centered(Vector2i(540, 0))
	_paste.grab_focus()


func _submit_exchange() -> void:
	var value: String = _paste.text.strip_edges()
	if value.is_empty():
		return
	if _exchange_kind == "join":
		_exchange.hide()
		if _connection.ask_join(value):
			return
		_exchange.popup_centered(Vector2i(540, 0))
	var accepted: bool = _orqwa.group_admit(value) if _exchange_kind != "join" else false
	if accepted:
		_request.text = tr("Copy join request")
		_exchange.hide()
		_paste.clear()
	else:
		_exchange_error.show()
		_exchange_error.text = tr(
			"That didn't work. Paste the entire invitation or request, or ask for a fresh one."
		)
	_update()


func _connection_started() -> void:
	_notice.text = ""
	_request.text = tr("Copy join request")
	_paste.clear()
	_update()


func _copy_invite() -> void:
	var invite: String = _status.get("invite", "")
	DisplayServer.clipboard_set(invite)
	_notice.text = tr("Invitation copied. Send it privately to your friends.")


func _copy_request() -> void:
	var reply: String = _status.get("reply", "")
	DisplayServer.clipboard_set(reply)
	_request.text = tr("Copied · send it to the organizer")
