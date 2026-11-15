# Group ride connections

How a group ride's computers find each other, stay connected, reconnect and part, and where
that can still go wrong. The decisions behind it are in
[ADR 0035](adr/0035-group-rides.md); riders' instructions are in the wiki's
[Group rides](https://github.com/ORQWA/ORQWA/wiki/Group-rides) guide. The code lives in
`core/orqwa-net` (`code`, `link`, `lobby`, `handover`) and `core/orqwa-app/src/group`.

## Connecting

1. **Invitation.** The organizer's ORQWA gathers its addresses (the public one a STUN server
   reports, and home-network ones for *Same home Wi-Fi* or *Some together*) and makes a fresh
   certificate. The invitation carries the ride, those addresses, the certificate's
   fingerprint and the connection choice, in code format version 19. An invitation from
   another version is refused with who must update (R248).
2. **Reply.** The friend's ORQWA gathers its own addresses and makes a certificate, or reuses
   the one it joined with (Join again). The reply carries the friend's name, the certificate's
   pin and their addresses. It waits up to **ten minutes** to be added (`REPLY_WINDOW`, R243):
   the friend tries to connect every five seconds for the first minute, then every 15 s, which
   also keeps their own NAT open.
3. **Adding the reply.** The organizer pastes it. ORQWA checks the ride, the addresses the
   connection choice allows, that it is not in use, and the bound of 16 replies a ride admits.
   For **one minute** (`PUNCH_WINDOW`) the organizer's computer then sends to the friend's
   addresses, which opens its own NAT and firewall for the friend's connection.
4. **Handshake.** QUIC with both certificates pinned (`orqwa-group/19`). The friend's first
   message is answered OK, wait (not added yet), replay (in use), expired or invalid. The
   organizer answers connection attempts at all only while a reply waits for its connection,
   or while it turns a removed rider away (below).
5. **Seat.** Hello and Welcome give the friend a seat: the one their certificate last had while
   it is free, otherwise the first free one.

Not connected in time, the organizer reads *Couldn't reach Ben's computer* with *How to check*;
the friend reads the reason in red beside **New reply** (#574).

## Reconnecting

A connection counts as lost after 30 s without packets (keep-alive every 10 s), or when a
stalled side closes it with the *reconnect* close code, so neither side takes it for a goodbye
(#434).

- The organizer **holds the seat for five minutes** (`RECONNECT_WINDOW`) and sends to the
  rider's last address every two seconds. The countdown waits while anyone is away.
- The friend tries every five seconds for **five minutes and 30 s** (`RECONNECT_TRYING`: the
  extra half minute is the longest the organizer may take to notice). **Reconnect now** tries
  at once. Their ride goes on alone; voice pauses.
- Back in time, the friend has the same seat. Otherwise the organizer releases the seat and the
  friend's group ends as lost.
- **A changed network** (another Wi-Fi, a cable, a new address) leaves the friend's sockets
  bound to addresses that are gone: nothing they send arrives. ORQWA checks every two seconds,
  starts reconnecting at once rather than after the 30 s timeout, stops sending from the dead
  sockets and offers Join again (R250). Everyone sees how long a friend is away; the organizer
  has a field for that friend's new reply.
- **Join again works while reconnecting**: the organizer adds the new reply, also while it
  still counts the friend as connected, as long as it brings new addresses; the stale
  connection is ended, the seat held, and the friend takes it back. Where the organizer's
  network lets the new address in, the friend is back before the reply is even added.

## Joining again

A friend whose connection was lost for good, or whose connecting failed, has **Join again**
(R248, #536): a fresh reply from the same invitation, with the same certificate, which the
organizer adds as any reply. A released certificate gets in again once added, in its old seat,
without using another of the 16 replies. A reply for a seat still held only adds addresses for
the organizer to send to. The friend's ride goes on meanwhile, and is taken up where it is if
the group still rides the same activity. **New reply** after a reply ran out is the same.

## How a connection ends

| Cause | On the wire | The friend reads | Join again |
|---|---|---|---|
| The friend leaves the group | plain close | – | – (the seat is freed at once) |
| Lost, not back within five minutes | idle timeout | *Group connection ended…* | yes |
| The organizer removes a connected friend | `REMOVED` close code | *The organizer removed you…* | no |
| The organizer removes a friend who is away | reconnection refused for 15 s (`TURN_AWAY`) | *The organizer removed you…* | no |
| The organizer ends the group | `ENDED` on every connection | *The organizer ended the group…* | no |
| The organizer leaves or is lost, with a fallback who agreed | old links end, the fallback takes over | rides on with the new organizer | yes, to the new organizer |
| The takeover fails | why the old link ended, recorded first | as that cause | only if lost |

During a ride, the ride screen tells each of these on the moments' card with their chime
(R249): a friend who dropped (*Lost the connection…*), who is back, or who left after
dropping; this rider losing the group, reconnecting, or their group ending with why. What
went wrong shows in red, who is back in green. The core compares successive group snapshots
(`group::notices`), so each change is told once.

A fallback takes over when the organizer leaves or stays unreachable for about a minute
(R187), and only if the friends it still reaches lost the organizer too: a fallback whose own
network dropped reaches nobody and does not take over (R251). Friends move to the links they prepared with it, take its invitation for Join again,
and answer it with the certificate they joined with; the new organizer asks the next fallback.

## Known issues

Nothing below is covered by tests on real networks: every test runs on loopback, so lost
packets and NAT behaviour are reasoned about, not measured.

1. **No relay.** Carrier-grade NAT, symmetric NAT on both sides or a firewall that drops
   unsolicited UDP keep friends out entirely; ORQWA only explains why.
2. **A close code can be lost.** QUIC sends the closing packet without acknowledgement and
   repeats it only briefly. Lost, the friend notices only from the idle timeout and reads a lost
   connection: they try to reconnect for five and a half minutes, then get Join again. This
   matters most for `ENDED`, as the organizer's ORQWA is gone right after.
3. **An organizer who crashes or loses the internet for good**, without a fallback, looks like
   a lost connection too, with the same five and a half minutes and a Join again nobody answers.
4. **Join again within a minute of a failed connect** is refused as *already added* while the
   organizer's computer still waits for the old reply.
5. **Removed while offline longer than 15 s**, a friend reads a lost connection later and gets a
   Join again the organizer will not answer.
6. **The certificate is kept in memory only.** After restarting ORQWA a friend gets a new
   seat, and each restart uses one of the ride's 16 replies; many restarts or newcomers can
   fill them.
7. **An old reply added again** after a seat was released is accepted, with addresses that may
   have changed: the organizer sends to stale ports, which usually still connects, as the friend
   starts the connection.
8. **Within the ten minutes a reply waits**, a mobile or carrier-grade NAT may change the
   friend's public port despite the tries every 15 s, and the organizer sends to the wrong one.
9. **A finished ride with its summary open** counts as still going: a friend joined again reads
   that the group waits for their ride to end.
10. **A takeover's links get one minute** to connect: a friend whose link fails loses the
    takeover and is offered Join again to the new organizer.
11. **A changed public address alone goes unnoticed.** ORQWA sees a network change only when
    the computer's own address goes away. A provider or carrier-grade NAT that changes only the
    public address or port still shows after the 30 s timeout, and reconnecting then fails
    unless the organizer's side lets the new address in; Join again works as usual.
12. **Builds without the close codes.** `REMOVED` and `ENDED` came without a protocol version
    change; a version 19 build without them reads a plain close and shows the generic ending.

Deciding about 1 to 3 is in [Open questions](OPEN-QUESTIONS.md#group-endings-a-friend-cannot-tell-2026-11-15).
