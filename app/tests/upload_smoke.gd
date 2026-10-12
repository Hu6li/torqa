extends SceneTree
## Headless checks of uploading rides to training platforms (R29): the platforms on the rider's
## card, the dialog that connects an account, and a ride's upload menu. Needs no network.
## Run: godot --headless --path app -s res://tests/upload_smoke.gd

const MAIN_SCENE: String = "res://scenes/main.tscn"

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	await _uploads()
	if not _failed:
		print("UPLOAD SMOKE TEST PASSED")
	quit(1 if _failed else 0)


## Rides go up to the rider's training platforms (R29): every platform is listed on the
## rider's card with the way to connect it, connecting asks for what the platform needs, and
## a ride's upload menu connects first, uploads, or leads to the ride's page.
func _uploads() -> void:
	var main: Control = (load(MAIN_SCENE) as PackedScene).instantiate()
	root.add_child(main)
	await process_frame
	var torqa: TorqaApp = main.get_node("Torqa")
	var tab: ProfileTab = ProfileTab.new()
	root.add_child(tab)
	tab.bind(torqa)
	var unfolded: Dictionary = tab.get("_unfolded")
	unfolded["nobody-yet"] = true
	tab.call("_show_riders", [{"id": "nobody-yet", "name": "Nobody"}], "nobody-yet")
	var cards: VBoxContainer = tab.get("_cards")
	var texts: Array[String] = []
	for label: Node in cards.find_children("*", "Label", true, false):
		texts.append((label as Label).text)
	_check(
		"ACCOUNTS" in texts and "intervals.icu" in texts and "Not connected" in texts,
		"the rider's card lists the platforms: %s" % [texts]
	)
	var connect: Button = _button_with_text(cards, "Connect…")
	_check(connect != null, "a platform not connected can be connected")
	if connect != null:
		connect.pressed.emit()
	var dialog: AccountDialog = tab.get("_account_dialog")
	var edits: Array[Node] = dialog.find_children("*", "LineEdit", true, false)
	var link: LinkButton = dialog.get("_link")
	_check(
		(
			dialog.visible
			and dialog.title == "Connect intervals.icu"
			and edits.size() == 1
			and (edits[0] as LineEdit).secret
			and link.uri == "https://intervals.icu/settings"
		),
		"connecting intervals.icu asks for the API key, hidden, and where to find it"
	)
	dialog.get_ok_button().pressed.emit()
	var status: Label = dialog.get("_status")
	_check(
		dialog.visible and status.visible and not status.text.is_empty(),
		"an empty key is not sent, and the dialog says so"
	)
	dialog.hide()
	tab.free()

	var history: HistoryScreen = main.get_node("HistoryScreen")
	var ride: Dictionary = {
		"path": "/nowhere/torqa-20261012-071500.fit",
		"name": "Morning loop",
		"route": "Gurten",
		"start_unix_s": 0,
		"uploads": {},
	}
	history.set("_rides", [ride])
	history.set("_selected", 0)
	history.call("_fill_upload_menu")
	var upload: MenuButton = history.get("_upload_button")
	var menu: PopupMenu = upload.get_popup()
	_check(
		menu.item_count == 1 and menu.get_item_text(0) == "Connect intervals.icu and upload…",
		"a platform not connected is connected first: %s" % menu.get_item_text(0)
	)
	ride["uploads"] = {"intervals": "https://intervals.icu/activities/i1"}
	history.call("_fill_upload_menu")
	_check(
		menu.get_item_text(0) == "View on intervals.icu",
		"an uploaded ride leads to its page: %s" % menu.get_item_text(0)
	)
	main.free()


func _button_with_text(node: Node, text: String) -> Button:
	for button: Node in node.find_children("*", "Button", true, false):
		if (button as Button).text == text:
			return button
	return null


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("UPLOAD SMOKE TEST FAILED: " + what)
		_failed = true
