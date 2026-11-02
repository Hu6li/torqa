class_name GroupInviteCard
extends VBoxContainer
# gdlint: disable=max-line-length
## The organizer's invitation (R195): one for every friend, sent outside ORQWA by chat, email
## or text message; with a warning when friends at other homes probably cannot reach them.

const HOSTING_GUIDE: String = "https://github.com/ORQWA/ORQWA/wiki/Group-rides#who-should-create-the-ride"

var _code: GroupCodeRow = GroupCodeRow.new()
var _reach: VBoxContainer = VBoxContainer.new()
var _reach_text: Label = Label.new()


func _ready() -> void:
	add_theme_constant_override("separation", 10)
	var title: Label = Label.new()
	title.text = tr("Invite friends")
	title.add_theme_font_size_override("font_size", 20)
	add_child(title)
	var how: Label = Label.new()
	how.text = tr("Send this invitation by chat, email or text message.")
	how.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	how.add_theme_color_override("font_color", UiTheme.MUTED)
	add_child(how)
	_code.qr_title = tr("Invitation")
	_code.qr_hint = tr("Scan with your phone, then send the text to your friends.")
	add_child(_code)
	_reach_text.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_reach_text.add_theme_color_override("font_color", UiTheme.WARNING)
	_reach.add_child(_reach_text)
	var check: Button = Button.new()
	check.text = tr("How to check")
	check.flat = true
	check.add_theme_color_override("font_color", UiTheme.ACCENT)
	check.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	check.pressed.connect(func() -> void: OS.shell_open(HOSTING_GUIDE))
	_reach.add_child(check)
	add_child(_reach)


## Shows `invite`, empty while there is none or it may not be sent, and `warning` when friends
## at other homes probably cannot reach the organizer (`GroupRidesTab.reach_warning()`).
func show_invite(invite: String, warning: String) -> void:
	_code.show_code(invite)
	_reach_text.text = warning
	_reach.visible = not warning.is_empty()
