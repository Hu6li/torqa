class_name GroupPingChecks
extends RefCounted
## Friend connection readings stay inside the group roster (R235).


static func check(tree: SceneTree, verify: Callable) -> void:
	var app: OrqwaApp = OrqwaApp.new()
	tree.root.add_child(app)
	var group: GroupRidesTab = GroupRidesTab.new()
	group.compact = true
	group.bind(app)
	tree.root.add_child(group)
	group.set_process(false)
	var roster: Array[Dictionary] = [
		{
			"id": 0,
			"name": "Organizer",
			"ready": true,
			"me": false,
			"organizer": true,
			"ping_ms": 40
		},
		{"id": 1, "name": "You", "ready": true, "me": true, "ping_ms": 40},
		{"id": 2, "name": "Friend", "ready": true, "me": false, "ping_ms": 180},
	]
	var status: Dictionary = {"phase": "riding", "host": false, "voice": true, "roster": roster}
	group._show_status(status)
	verify.call(not group._pings.has(1), "the roster never shows a self ping")
	verify.call(group._pings[0].text == "Ping 40 ms", "the organizer row shows our link")
	verify.call(group._pings[2].text == "Ping 180 ms", "a friend has their own ping")
	verify.call(
		"this friend" in group._pings[2].tooltip_text, "friend ping explains the organizer link"
	)
	var label: Label = group._pings[2]
	var button: Button = group._voice_buttons[2]
	roster[2]["ping_ms"] = 220
	group._show_status(status)
	verify.call(
		group._pings[2] == label and label.text == "Ping 220 ms", "live ping updates keep rows"
	)
	verify.call(group._voice_buttons[2] == button, "ping updates preserve voice buttons")
	roster[2]["away"] = true
	group._show_status(status)
	verify.call(group._pings[2].text == "Ping —", "reconnecting friends have no old reading")
	roster[2].erase("away")
	roster[2].erase("ping_ms")
	group._show_status(status)
	verify.call(group._pings[2].text == "Ping —", "an unknown ping never appears as zero")
	status["reconnect_s"] = 120.0
	group._show_status(status)
	verify.call(group._pings[0].text == "Ping —", "losing our organizer link clears the ping")
	group.free()
	app.free()
	await tree.process_frame
