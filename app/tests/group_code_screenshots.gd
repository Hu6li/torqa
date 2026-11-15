class_name GroupCodeScreenshots
extends RefCounted
## Screenshots of group codes for the screenshot run (scripts/screenshots.sh).


## Codes sent outside ORQWA (R195): a joiner's reply with the time left and after it ran out,
## the organizer's invitation, a friend connecting, one never reached (R243), a wrong paste and
## the QR code.
static func capture(tree: SceneTree, group: GroupRidesTab, invite: String, out: String) -> void:
	var root: Window = tree.root
	group._joined = {"organizer": "Alex", "mode": "internet"}
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		group._show_status({"phase": "connecting", "reply": invite, "reply_s": 552.0})
		await tree.create_timer(0.4).timeout
		root.get_texture().get_image().save_png(out.path_join("group-joining-" + locale + ".png"))
	TranslationServer.set_locale("en")
	var blocked: String = (
		"The direct UDP connection did not finish in time. Check the codes, UDP firewall, "
		+ "IPv6 or LAN mode; some networks cannot connect without a relay."
	)
	group._show_status({"phase": "failed", "reply_expired": true, "error": blocked})
	await tree.create_timer(0.3).timeout
	root.get_texture().get_image().save_png(out.path_join("group-joining-expired-en.png"))
	group._joined = {}
	TranslationServer.set_locale("en")
	var roster: Array[Dictionary] = [
		{"id": 0, "name": "Rider", "ready": false, "me": true, "organizer": true},
		{"id": 1, "name": "Alex", "ready": true, "me": false},
	]
	group._show_status(
		{"phase": "waiting", "host": true, "invite": invite, "admitting": 1, "roster": roster}
	)
	group._add._field.text = invite
	group._add._submit()
	await tree.create_timer(0.4).timeout
	root.get_texture().get_image().save_png(out.path_join("group-invite-en.png"))
	group._add._field.clear()
	# An added friend whose connection never arrived is named, with the guide (#574).
	group._add.show_refusal("", 0, "")
	group._add.show_refusal("unreached", 1, "Ben")
	await tree.create_timer(0.3).timeout
	root.get_texture().get_image().save_png(out.path_join("group-unreached-en.png"))
	group._add._edited("")
	group._invite._code._qr.pressed.emit()
	await tree.create_timer(0.4).timeout
	root.get_texture().get_image().save_png(out.path_join("group-invite-qr-en.png"))
	group._invite._code._dialog.hide()
	group._show_status({})
	group._join._field.text = "orqwa:nothing"
	group._join._submit()
	await tree.create_timer(0.3).timeout
	root.get_texture().get_image().save_png(out.path_join("group-join-error-en.png"))
	group._join.clear()
