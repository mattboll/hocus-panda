# Mail in the bar: what matters (#656, direction 1)

Status: **Draft — not approved.** Nothing is built before the maintainer
approves the mockup and answers the open questions (§14). The mockup:
the maintainer's `hocus-panda-tools/mockups/s29-mail`, its scenario
switcher playing the scenarios of §13 by their numbers.

The need is issue #656's first point: be told what matters in mail, and
only that. Someone who lives in webmail tabs checks them whenever they
think of it, still answers hours later than they could, and the
notification mail of their agents and forges drowns the rest. This page
puts a lens over the accounts' inboxes: the bar says, per account,
whether something is waiting for the user; a bubble interrupts only for
a person writing to them, or for a forge asking them something or
saying something broke; the rest is counted, folded, and emptied in a
batch. Every action is a Gmail action, the same on the phone.

Mail is touchy: a desktop that loses, sends or hides a mail costs more
than it saves. **Safety and predictability come first** (§10): the lens
can do four reversible things to a mailbox and nothing else, each
undoable, each explained.

## 1. Scope

**In**: Google accounts (Workspace and gmail.com) that GNOME Online
Accounts already holds, each kept apart; their inbox, read over IMAP;
a mark per account in the bar; a popover per account; bubbles for what
matters, each saying why; "Lu et archivé" one by one or in a batch;
"Plus tard" (snooze) as labels the phone sees; "Ouvrir dans Gmail";
Settings › Courrier.

**Not in**: a mail window, reading a mail's body in the desktop,
writing, replying, sending, drafts, search, contacts' history, an
assistant preparing replies (#656's later directions: §15 keeps room for
them); other providers (IMAP with a password, Outlook, Fastmail); mail
outside the inbox; Gmail's own snooze (no API sets it); filters or the
account's settings.

## 2. Words

| Word (UI) | What it is |
|---|---|
| « Pour toi » | A conversation that interrupts: a bubble, the bar's count |
| « À voir » | Shown in the popover, no bubble, not counted in the bar |
| « Le reste » | Folded and counted by kind; emptied in a batch |
| « Lu et archivé » | Marked read and taken out of the inbox (Gmail's Archive) |
| « Plus tard » | Taken out of the inbox until a time, then brought back unread |
| « Revenu » | A conversation brought back by « Plus tard » |

The unit is the **conversation** (Gmail's thread, `X-GM-THRID`), as on
the phone. A conversation's verdict is the strongest of its messages
still in the inbox and unread (Pour toi over À voir over Le reste).

## 3. Accounts

- **Where they come from**: Online Accounts, as the calendar's do
  ([day.md](day.md), "Google Calendar"): the session bus the compositor
  names, never another; an `org.gnome.OnlineAccounts.Account` of
  `ProviderType` `google` with its mail on (`MailDisabled` false) and
  `OAuth2Based`. The token is `GetAccessToken`'s, asked for each
  connection and on a refusal, **never written anywhere**. No OAuth
  client of ours, no review by Google: GNOME's client asks for
  `https://mail.google.com/` already.
- **Access**: IMAP over TLS to `imap.gmail.com:993`, `AUTHENTICATE
  XOAUTH2` with that token; Gmail's extensions (`X-GM-LABELS`,
  `X-GM-THRID`, `X-GM-MSGID`, `X-GM-RAW`), `IDLE` for push. Two
  connections an account at most: one idles on the inbox, one acts.
- **Each kept apart**: one worker thread an account, its own rules'
  results, its own row in the bar, its own tab in the popover, its own
  bubbles (named by the account). A batch never spans two accounts; no
  list ever mixes them.
- **Three ways to show** (`show`): « Barre et bulles » (`bar`), « Sur
  demande » (`asked`: no row, no bubble; read only while its tab is
  open in the popover, under « Autres ▾ »), « Éteint » (`off`:
  never read). **Every account is off until the user turns it on** in
  the first setup (§13, S1) or in Settings.
- **Consent, per account**: an account is read only once its consent is
  given (`consented`, the date), on a sheet that says in plain words
  what the lens reads and what it may do (§10.1). Without it, nothing
  is fetched, not even the headers.
- **Read-only** (`write = false`): the lens reads and interrupts, its
  actions are hidden; the IMAP connection only ever `EXAMINE`s.
- **Name and colour**: the name the bar shows (« Travail », « Perso »),
  by default the address's domain for a Workspace account and « Gmail »
  for gmail.com, changed in the setup; a colour from the accents'
  (bar.md §13) not yet taken.

## 4. The bar

- **Full form, the group « COURRIER »**: after To do and before the
  agents (bar.md §4's column). An 18 px header, "COURRIER" (Manrope 700,
  11.5 px, spaced 0.1 em, `text.dim`); then one 26 px line per `bar`
  account: an 18 px tile (radius 5, the account's initial on its
  colour), its name (13 px, cut), and on the right:
  - something « Pour toi », unread: a pill in `state.wait` (`#F5B44A`,
    the user is waited for, as by an agent), the count in Unbounded 600
    11 px on `#0B1020`, then the oldest one's age (12 px, `text.dim`:
    "14 min", "2 h", "hier");
  - only « À voir »: "3 à voir" (12 px, `text.faint`);
  - nothing: "calme" (12 px, `text.faint`);
  - a problem: a 14 px warning in `state.stuck` and "à reconnecter",
    "hors ligne" (12 px).
  A click on a line opens the popover on that account. Short of room,
  the group gives way after Today and before To do: its lines fold into
  one, "COURRIER" and the pills side by side (each its account's
  colour on its left). No `bar` account: no group.
- **Mini form**: one 48 px item under To do's: a 20 px envelope, and
  under it one 12 px line per `bar` account, its colour's 6 px dot and
  its unread « Pour toi » count (Manrope 700, 11 px), a faint dot when
  calm. A hover opens the popover as the mini items do (bar.md §9).
- **Motion**: a new « Pour toi » makes its pill grow from 0.6 to 1 with
  a slight overshoot (220 ms) and the line glow `rgba(245,180,74,0.10)`
  for 1.2 s; a count going down just changes. No halo (the halo is the
  agents' and the meetings'). None of it with reduced motion.

## 5. The popover

360 px (a list needs it; the weather's is 320), the popovers' glass and
mechanics (bar.md §9), its height to its content, 560 px at most, its
lists scrolling inside.

- **Head**: the accounts as a segmented control, one segment a `bar`
  account (its tile, its name, its unread « Pour toi » count), then
  « Autres ▾ » when an `asked` account exists. The segment
  opened is the one clicked in the bar, else the last.
- **Banners**, under the head, one at a time, the most serious first:
  the account to reconnect (S14), the administrator's block (S15),
  offline (S16), observing (S18).
- **« POUR TOI »**: one 52 px row a conversation, newest first: the
  sender's initial on a colour from their address (26 px), the sender
  (13.5 px 700; « Camille Roux », « GitHub · acme/api »), the subject
  (12.5 px, one line), a kind chip (« Personne », « Revue demandée »,
  « Mention », « CI rouge sur main », « Sécurité »), the age. Unread ones
  first, then the read ones at 70 %.
- **« REVENUS »**: what « Plus tard » brought back today, the same rows,
  "prévu 8:00" in place of the age.
- **« À VOIR »**: the same rows.
- **« LE RESTE · N »**: folded. One 34 px line a kind, its count on the
  right: « GitHub · dépôts suivis », « GitLab », « Robots et agents »,
  « Lettres d'info », « Notifications de services », « Autres
  automatiques ». A line opens to its conversations (28 px, sender and
  subject). Each line and each conversation has a box; the kind's box
  takes its conversations.
- **On a row**, under the pointer or the keyboard's focus: ✓ « Lu et
  archivé », ⏰ « Plus tard », ↗ « Ouvrir dans Gmail », ⓘ « Pourquoi ? ».
  A click on the row itself is « Ouvrir dans Gmail ». A row of « Pour
  toi » or « À voir » has a box too, shown once a box is ticked.
- **The batch bar**, while something is ticked, over the footer: "N
  conversations", « Lu et archivé (N) » (the accent), « Plus tard… »,
  « Tout décocher ». More than 100 ticked: the button is inert and says
  "100 au plus à la fois".
- **« Pourquoi ? »** opens under the row: the verdict, one sentence
  (§6's reason), what it read ("Cc : review_requested@noreply.github.com",
  "Expéditeur : renovate[bot]"), and « Ne plus m'interrompre pour
  Camille Roux » / « M'interrompre pour … » (a person), or « Changer
  cette règle… » (Settings › Courrier, its row lit).
- **The toast** after an action, at the popover's bottom: `#262C3C`,
  "3 conversations lues et archivées", « Annuler », a 2 px line counting
  down `undo-seconds` (10 s). The popover closed before it ends: a bubble
  holds the rest of the countdown, « Annuler » its action.
- **Footer**: « Ouvrir Gmail » (the account's inbox), « Ce qui a été
  fait » (the journal, §10.4), « Réglages du courrier… ».
- **Keyboard**: arrows move between rows, `Space` ticks, `E` « Lu et
  archivé », `L` « Plus tard », `Enter` opens in Gmail, `?` « Pourquoi ? »,
  `Ctrl+Z` undoes the last action while its toast shows.

## 6. The rules

Fixed rules, in this order, the first that applies deciding. No
learning, no score: the same mail always gets the same verdict, and its
reason says which rule. Each kind's verdict is a setting (§9). Headers
only; the rules never read a body.

1. **From the user** (the account's address or its aliases, Gmail's
   "send as"): « Le reste » ("Écrit par toi").
2. **Robots and agents**: a sender whose name ends in `[bot]`, GitLab's
   bot users (`project_…_bot…`, `group_…_bot…`), or a name the user
   listed as an agent (`agents`): « Le reste » ("Écrit par un robot :
   renovate[bot]"). An agent posting under the user's own account looks
   like the user: rule 1 or GitHub's `your_activity` takes it.
3. **GitHub** (`From` `notifications@github.com`, its reason in the
   `Cc`, `…@noreply.github.com`; `List-Id` the repository):
   - `review_requested`: « Pour toi » ("Camille Roux te demande une revue")
   - `mention`: « Pour toi »; `team_mention`: « À voir »
   - `assign`: « À voir »
   - `security_alert`: « Pour toi »
   - `ci_activity` (runs the user triggered, theirs or their agents'): a
     failed run on the repository's default branch (`main`, `master`,
     or a branch listed in `main-branches`) « Pour toi » ("CI rouge sur
     main"); a failed run elsewhere « À voir »; a passing run « Le reste »
   - `author`, `comment`, `state_change` on a conversation the user is
     in, written by a person (not rule 2): « Pour toi » ("Léa a répondu
     sur ta PR")
   - `subscribed` (a watched repository), `push`, `your_activity`:
     « Le reste »
4. **GitLab** (`X-GitLab-NotificationReason`; pipelines by
   `X-GitLab-Pipeline-Id` and their status and ref headers):
   `review_requested`, `mentioned` « Pour toi »; `assigned` « À voir »;
   a failed pipeline on the default branch « Pour toi », elsewhere « À
   voir »; `own_activity`, `subscribed` « Le reste »; no reason given:
   « À voir » (rule 8).
5. **Mailing lists** (`List-Id` not a forge's): « À voir » when written
   by a person and the user is in `To` or `Cc` by name; else « Le reste »
   (« Lettres d'info »).
6. **Automated mail** (`Auto-Submitted` other than `no`, `Precedence:
   bulk`/`list`, `List-Unsubscribe`, a `noreply`/`no-reply` sender, or
   Gmail's own categories Promotions, Social, Updates, Forums as
   `X-GM-RAW` finds them): « Le reste » (« Notifications de services »,
   « Lettres d'info »).
7. **A person writing to the user**: a sender that none of the above
   took, the user in `To` or `Cc`: « Pour toi » ("Camille Roux t'écrit
   directement"). `people = "known"` keeps that to people the user wrote
   to before (read from the sent mail's recipients, headers only); the
   others « À voir », marked « nouveau ».
8. **Unsure** (nothing above applies, a header that does not parse, a
   forge's mail without its reason): **« À voir », never « Le reste »**.
   When unsure, show.

A person override (`[mail] people-interrupt`, `people-quiet`) comes
before rule 3: « M'interrompre pour … » makes a sender « Pour toi »
whatever they send but robots' mail; « Ne plus m'interrompre » makes
them « À voir ».

**What the lens reads**: the inbox's conversations of the last 30 days,
the newest 1000 at most; older ones are not read, and the popover says
how many ("+ 2 340 plus anciens, pas regardés"). Headers fetched with
`BODY.PEEK[HEADER.FIELDS (…)]` (a peek never marks read), the flags,
the labels.

## 7. Bubbles

Through the desktop's own notifications (the shell's stack,
[notifications.md](notifications.md)), application « Courrier », so that
do not disturb, the screen shared (5.11) and Settings › Notifications'
per-application levels hold them as any other.

- **Only for « Pour toi »**, once a conversation becomes so (a new
  message, a « Revenu »), never again for the same message.
- **Its look**: "Courrier · Travail · maintenant"; the title the sender
  (or "Revue demandée · acme/api #482"); the body the subject, then the
  reason in its own words ("Pourquoi : Camille t'écrit directement");
  « Ouvrir », « Plus tard », « Lu et archivé » (the last two absent on a
  read-only account). Normal urgency, never critical: a mail is not an
  alarm.
- **A forge's** « Ouvrir » opens the pull request, the issue or the run
  (the link GitHub and GitLab put in the mail, read from its first text
  part's first 4 KB when the bubble is made, peeked, kept in memory);
  the mail itself stays as it is.
- **Many at once**: more than 3 within two minutes on one account make
  one bubble, "3 conversations pour toi", its click opening the popover.
- **After an absence** (the shell starting, back online, the machine
  resuming): no bubble a mail; one, "Pendant ton absence : 2 pour toi".
- **Withdrawn** when the conversation is read, archived or snoozed
  anywhere, the phone included (S13).

## 8. Plus tard

A snooze Gmail's phone app sees, made only of labels, since nothing can
set Gmail's own: **the conversation is taken out of the inbox and given
the label `Plus tard/2026-11-21 08:00`**; at that time it is brought back
into the inbox, unread, and the label taken off. The label's name is the
schedule: nothing is kept on the computer, a reinstall or a second
computer finds the same, and the phone shows each snoozed mail under
« Plus tard » in its labels.

- **The picker** (from a row, a bubble or the batch bar), its choices
  with their date in full:
  - « Dans 2 min » (`nudge-minutes`): stays in the inbox, marked
    unread; the bar brings it back to « Pour toi » and makes a bubble
    when the time comes. The computer's alone (nothing for the phone to
    see but the unread mark); kept in `$XDG_STATE_HOME/hocuspanda/mail/nudges`.
  - « Ce soir » (`evening`, 18:00), not after it; « Demain matin »
    (`morning`, 8:00); « Lundi matin » from Friday on; « Choisir une
    date… » (a month's calendar, as Day.5's, and the hour).
  - **« Le jour J »**, only when the mail says a date: a reservation
    (schema.org's `EventReservation`, `LodgingReservation`,
    `FlightReservation`, … in its JSON-LD or microdata, as booking mail
    carries for Gmail's own cards) or a calendar invitation (its
    `text/calendar` part's `DTSTART`). "Le jour J · sam. 14 mars, 8:00 ·
    arrivée à l'hôtel, d'après le mail". The body is read for that only,
    when the picker opens, peeked, and dropped once the date is found.
  - Under the choices: "Rangé dans « Plus tard/2026-11-21 08:00 »,
    visible sur ton téléphone."
- **Bringing back**: the worker reads the `Plus tard/…` labels as it
  starts and every hour, and wakes at the next one due (wall clock: the
  time is the one the user read). Due: `X-GM-LABELS` `+\Inbox`,
  `-Plus tard/…`, `-\Seen` on the messages still under the label (and
  not in the trash or spam), then the label removed once nothing holds
  it. It shows under « Revenus », with its bubble.
- **The computer off at that time**: brought back as soon as the
  account is read again, the row saying "prévu hier 8:00". Meanwhile the
  phone shows it under its label, not in its inbox. A conversation that
  comes back is not on top of the phone's inbox (Gmail orders by the
  last message's date): the bar's « Revenus » and the bubble say it. To
  bring it back with the computer off, an opt-in script in the account
  (§14, Q3; lot D).
- **Changed elsewhere**: the label taken off on the phone, or the mail
  put back in the inbox there: it is no longer snoozed, nothing is done.
  A `Plus tard/…` label whose name is not a date the lens wrote is left
  alone, and said once in the journal.
- **Label names**: `later-label` ("Plus tard", the language's word at the
  first setup, then kept: a label already on mails is never renamed),
  `/`, the date and the hour, `YYYY-MM-DD HH:MM`, in the system's zone.

## 9. Settings › Courrier

Section `mail` (`hocus settings mail`), in TRAVAIL after Notifications,
an envelope. Under the title: "Ce qui t'interrompt dans ton courrier, et
ce que la barre peut y faire." Every change is written at once, with its
toast and its undo (settings.md); the defaults live in
`packaging/config.toml` as every default does.

| Group | Row | Control | Key (default) |
|---|---|---|---|
| COMPTES | One card per Google account of Online Accounts: its tile, its address, its state ("Lu à 14:02 · 412 conversations regardées", "À reconnecter", "Bloqué par l'administrateur") | | |
| | Dans la barre | segmented « Barre et bulles \| Sur demande \| Éteint » | `[[mail.accounts]] show` (`"off"`) |
| | Nom dans la barre | field | `name` (the domain, « Gmail ») |
| | Peut ranger | switch: off is read-only | `write` (`true`) |
| | Couleur | the accents' swatches | `color` |
| | No account: "Ajoute un compte Google dans les comptes en ligne", running `gnome-control-center online-accounts` | | |
| CE QUI T'INTERROMPT | Une personne qui t'écrit | menu « N'importe qui \| Les gens à qui tu as déjà écrit \| Personne » | `[mail] people` (`"anyone"`) |
| | Listes de diffusion | menu (below) | `[mail.kinds] lists` (`"show"`) |
| GITHUB ET GITLAB | Revue demandée | menu « M'interrompre \| Montrer sans bulle \| Compter seulement » | `[mail.kinds] review-requested` (`"interrupt"`) |
| | Mention | same | `mention` (`"interrupt"`) |
| | Mention d'une équipe | same | `team-mention` (`"show"`) |
| | Assigné | same | `assigned` (`"show"`) |
| | CI rouge sur la branche principale | same | `ci-main` (`"interrupt"`) |
| | CI rouge ailleurs | same | `ci-other` (`"show"`) |
| | Alerte de sécurité | same | `security` (`"interrupt"`) |
| | Une personne répond où tu participes | same | `comment-mine` (`"interrupt"`) |
| | Un dépôt que tu surveilles | same | `watched` (`"fold"`) |
| | Branches principales | chips | `[mail] main-branches` (`["main", "master"]`) |
| | Comptes d'agents ("Leurs messages comptent comme ceux des robots") | chips, « + Ajouter » | `[mail] agents` (`[]`) |
| PLUS TARD | Le matin | hour | `[mail.later] morning` (`"08:00"`) |
| | Le soir | hour | `evening` (`"18:00"`) |
| | « Dans un moment » | menu 2, 5, 10, 15 min | `nudge-minutes` (`2`) |
| | Ramener même ordinateur éteint | « Installer dans le compte… » (lot D) | `script` (`false`) |
| PRUDENCE | Observer d'abord ("Pas de bulle : la barre dit ce qui t'aurait interrompu") | switch, its end date | `[mail] observe-until` (the first consent + 7 days) |
| | Temps pour annuler | menu 5, 10, 30 s | `undo-seconds` (`10`) |
| | Ce qui a été fait | « Voir le journal » | |

The verdict words: `interrupt` « M'interrompre », `show` « Montrer sans
bulle », `fold` « Compter seulement ». The page's note, plain: "Ce que
Hocuspanda lit. Les en-têtes des mails de ta boîte de réception (qui,
à qui, l'objet, la date, les libellés), jamais le texte, sauf la première
ligne pour une bulle, le lien d'une notification GitHub ou GitLab, et la
date d'une réservation quand tu choisis « Le jour J ». Rien n'est gardé
sur l'ordinateur à part le journal de ce qui a été fait. Il n'envoie
rien, ne supprime rien. La connexion passe par tes comptes en ligne :
Hocuspanda ne garde aucun mot de passe."

Keys without a row: `[[mail.accounts]] address` (which account of
Online Accounts) and `consented` (the date of the setup sheet's
« D'accord pour ce compte », never written by hand: without it the
account is not read whatever `show` says), `[mail] people-interrupt`
and `people-quiet` (the addresses « Pourquoi ? »'s person buttons write),
`[mail.later] later-label`.

## 10. Safety

What the lens can do and never does. **To approve as written**, line by
line: this is the part the maintainer signs.

### 10.1 It never

- **deletes** a mail: never the `\Deleted` flag, never `EXPUNGE`, never
  the trash (`\Trash`);
- **sends** anything: no SMTP in the crate, no `APPEND`, no draft;
- **marks spam**, or takes a mail out of spam or the trash;
- **moves or copies**: no `MOVE`, no `COPY`; Gmail's labels only;
- **touches** another label, a star, « Important », a category, a
  filter, the account's settings;
- **acts on a mail it did not show**: an action names the messages
  listed when the user acted (their `X-GM-MSGID`), never a search, a
  label or "all";
- **acts by itself**: no rule archives, reads or snoozes. Rules only
  choose what interrupts. The one write it makes unasked is bringing
  back what the user snoozed, at the time they chose;
- **hides when unsure**: a mail no rule is sure of is « À voir »;
- **keeps a token** or a body: tokens are Online Accounts', asked each
  time; bodies are read in the three cases this page names (a bubble's
  first line, a forge's link, « Le jour J »), peeked, kept in memory;
- **queues** an action for later: offline, an action is refused and says
  so (S16); a snooze's return is a reading of the labels, not a queue.

### 10.2 It only ever writes these

The crate has one type for what it may write to a mailbox, and its IMAP
layer builds commands from that type alone (no other command that
writes exists in it):

| Write | IMAP | Undone by |
|---|---|---|
| Mark read, mark unread | `UID STORE ±FLAGS (\Seen)` | the other |
| Take out of the inbox, put back | `UID STORE ±X-GM-LABELS (\Inbox)` | the other |
| Add, remove its own label | `UID STORE ±X-GM-LABELS ("Plus tard/…")` | the other |
| Make its own label, remove it empty | `CREATE`, `DELETE` of `Plus tard/<date>` only, checked empty just before | — |

Gmail shows each the same in its web and phone apps: an archived mail
is in « Tous les messages », a snoozed one under its label. Nothing it
does needs the computer to be undone.

### 10.3 Before each write

- the account's consent is given and `write` is on; otherwise the
  connection only `EXAMINE`s (read-only for the server itself);
- the messages are still where they were shown (in the inbox, or under
  the label); one that moved meanwhile is left out, and said;
- **a conversation that got a new message since it was shown is left in
  the inbox** ("1 conversation a reçu un nouveau message : laissée dans
  ta boîte");
- 100 conversations at most a batch, one write at a time an account;
- a partial failure says exactly what was done, and the undo undoes
  that part.

### 10.4 After each write

- the toast and its « Annuler » for `undo-seconds`, then the **journal**
  (`$XDG_STATE_HOME/hocuspanda/mail/journal`, mode 0600, 30 days): the
  time, the account, the write, the messages' ids, the subject; each
  entry « Défaire » while its mails are still as it left them (an undo
  never touches a mail that moved since: one put in the trash on the
  phone stays there);
- the log says each write at info, ids only, never a subject.

### 10.5 Being sure before trusting it

- **Observe first**: from the first consent and for 7 days, no bubble;
  the popover's banner says "En observation jusqu'au 27 novembre · 6
  t'auraient interrompu aujourd'hui", each with its « Pourquoi ? », and
  « Activer les bulles » ends it at once. The actions work from the
  first day (the user does them, with their undo).
- **Every verdict explained** (§5, « Pourquoi ? »), in the bubble too.
- **Read-only per account** (`write = false`).

### 10.6 What stays on the computer

| What | Where | Kept |
|---|---|---|
| Headers, flags, labels of the inbox's conversations | memory | while the shell runs |
| The journal | `$XDG_STATE_HOME/hocuspanda/mail/journal` (0600) | 30 days |
| « Dans 2 min » | `$XDG_STATE_HOME/hocuspanda/mail/nudges` (0600) | until due |
| The addresses written to (`people = "known"` only) | memory | while the shell runs |

What leaves it: IMAP to Google, with Online Accounts' token. Opening a
mail is the browser's.

## 11. Ouvrir dans Gmail

`https://mail.google.com/mail/?authuser=<address>#all/<thread id, hex>`
through `xdg-open` (the compositor starts it): the default browser opens
it in a tab, on that account (`authuser` picks it whatever its place in
the browser's list). An open Gmail tab is not reused: no browser offers
that without an extension. Opening does not mark read: Gmail does, as it
shows the mail, and the lens follows (S13).

## 12. How it is built (for the lots)

- **The crate `hocus-mail`** (no smithay, no iced): the model
  (accounts, conversations, verdicts, reasons), the rules (pure
  functions of headers), the writes' type, the journal, the labels'
  schedule, an IMAP client of its own on `imap-codec` (MIT or
  Apache-2.0) over rustls: the commands it sends are the ones it wrote.
- **Its workers**: one thread an account in the shell, as Your day's
  (day.md), owned snapshots sent to the shell's `mail` module when they
  change; the account's IDLE connection renewed every 9 minutes and
  after a resume; Online Accounts followed by the same thread as the
  calendars'.
- **Words**: `shell.mail` and `settings.mail` in the catalogue.

## 13. Scenarios

Each a test where it can be (§16), and a state of the mockup; S1 to S26
elsewhere on this page.

1. **First setup.** Online Accounts holds a Workspace account and a
   gmail.com account. Settings › Courrier lists both, off. The user turns
   the work one to « Barre et bulles »: a sheet says, in plain words,
   what the lens will read, the four things it may do, what it never
   does, and that it observes for a week; « D'accord pour ce compte ».
   The lens checks IMAP at once and says "Lu à 14:02 · 412 conversations
   regardées". The personal account, the same, named « Perso ». Nothing
   in a terminal.
2. **A person writes (work).** Camille, a colleague, writes to the user
   alone. Within seconds (IDLE): the bubble "Camille Roux", the subject,
   "Pourquoi : Camille t'écrit directement"; the bar's Travail line shows
   the amber "1 · maintenant". The user does nothing: 40 minutes later
   the line says "1 · 40 min".
3. **A review is requested.** GitHub: Léa asks the user to review
   acme/api #482. The bubble "Revue demandée · acme/api #482", « Ouvrir »
   opens the pull request in the browser. The mail stays unread in the
   inbox until the user archives it.
4. **Main's CI breaks.** The user's merge (or their agent's push, under
   their account) fails on `main`: « Pour toi », "CI rouge sur main ·
   acme/web". The same workflow failing on an agent's branch: « À voir »,
   no bubble.
5. **An agent's bot comments.** `review-agent[bot]` comments on the
   user's pull request: no bubble, « Le reste », "Robots et agents". Its
   « Pourquoi ? » says "Écrit par un robot : le nom finit par [bot]".
   An agent posting as a machine account the user listed under
   « Comptes d'agents »: the same.
6. **Thirty watched-repository mails.** 30 `subscribed` mails arrive in
   an hour: no bubble, no count in the bar; « Le reste · 30 », « GitHub ·
   dépôts suivis 30 ».
7. **Plus tard, tomorrow.** On a mail to handle: ⏰ › « Demain matin ·
   ven. 21 nov., 8:00 ». The row goes, the toast says "Revient demain à
   8:00", the phone shows it under « Plus tard/2026-11-21 08:00 » and not
   in its inbox. Tomorrow at 8:00 it is back, unread, in « Revenus »,
   with its bubble.
8. **Plus tard, in six months.** A hotel's confirmation: the picker
   offers « Le jour J · sam. 14 mars, 8:00 · arrivée à l'hôtel, d'après
   le mail ». Chosen: the label `Plus tard/2027-03-14 08:00`. A concert
   ticket without a date the lens can read: « Choisir une date… ».
9. **Dans 2 min.** On a mail read too fast: « Dans 2 min ». It stays in
   the inbox, unread on the phone; two minutes later it is back in « Pour
   toi » with a bubble.
10. **Batch, then undo.** « Le reste · 39 »: the user ticks « GitHub ·
    dépôts suivis » (30) and « Lettres d'info » (2), « Lu et archivé
    (32) ». They go, the toast "32 conversations lues et archivées ·
    Annuler". The user sees a newsletter they wanted, « Annuler »: all 32
    are back in the inbox, unread as they were. Again without it: done,
    and in the journal.
11. **Opening in Gmail.** A click on Camille's row: the browser opens
    the conversation on the work account. Gmail marks it read; the bar's
    pill goes.
12. **The computer off when a « Plus tard » falls due.** Snoozed to 8:00,
    the laptop closed until 10:30: at 10:30 it comes back, "prévu 8:00",
    one bubble "Pendant ton absence : 1 revenu". Meanwhile, the phone
    showed it under its label only.
13. **The phone archives meanwhile.** Camille's mail archived on the
    phone: within seconds the row, the pill and the bubble (if still
    shown) go. Read on the phone: the pill goes, the row stays at 70 %.
14. **The account's token expired.** Online Accounts asks to sign in
    again: the Travail line says "à reconnecter"; the popover's banner
    "Ton compte Travail demande de se reconnecter" and « Ouvrir les
    comptes en ligne ». The last conversations shown stay, dimmed, their
    actions inert. Signed in again: read at once.
15. **The administrator blocks GNOME's access.** A Workspace whose
    administrator did not allow GNOME's application, or turned IMAP off:
    "Ton organisation bloque l'accès à la messagerie pour GNOME", what to
    ask the administrator (allow the application "GNOME" in the admin
    console's API controls; turn IMAP on), and, when the user is the
    administrator, « Ouvrir la console d'administration ».
16. **Offline.** The line says "hors ligne"; the popover shows the last
    list, "Hors ligne depuis 10:42"; ✓ and ⏰ are inert ("Pas de réseau :
    rien n'a été fait"). Back online: read again, one summary bubble.
17. **An account on demand.** A shared family account set « Sur
    demande »: no line in the bar, no bubble. The popover's « Autres ▾ ›
    Famille » reads it while its tab is open, the same lists.
    Off by default, as every account.
18. **The observing week.** The first 7 days: no bubble. The popover's
    banner "En observation · 6 t'auraient interrompu aujourd'hui"; each
    marked « aurait interrompu », with its « Pourquoi ? ». « Activer les
    bulles » ends it.

**What never happens**:

19. A « Lu et archivé » of a conversation where a new reply came after
    it was shown: the new reply stays in the inbox, and the toast says so.
20. « Annuler » after the phone put one of the mails in the trash: that
    one stays in the trash; the others come back; the toast says "1 mail
    est à la corbeille : laissé".
21. A batch where the server fails half way: "20 sur 34 rangées ; les
    autres n'ont pas bougé", « Annuler » for the 20.
22. A rule never archives: 300 robot mails in a night leave 300 mails in
    the inbox, counted in « Le reste ».
23. A mail the rules cannot read (a broken header, an unknown forge's
    format): « À voir », its « Pourquoi ? » "Pas sûr : montré".
24. A label `Plus tard/someday` made by hand on the phone: left alone,
    one line in the journal.
25. Two computers on the same account bring back the same snooze: the
    second finds nothing under the label and does nothing.
26. Any IMAP command outside §10.2's table: there is none in the crate;
    the tests' server fails the test on one (§16).

## 14. Open questions for the maintainer

Each with what this draft proposes.

- **Q1. Observe first?** Proposed: yes, 7 days without bubbles, « Activer
  les bulles » ending it at any time; the actions available from day one.
- **Q2. Which people interrupt?** Proposed: anyone writing to you
  directly (`To`/`Cc`, not through a list), « nouveau » shown for a
  first-time sender; « Les gens à qui tu as déjà écrit » as the setting.
- **Q3. A snooze due with the computer off.** Proposed: the desktop only
  (brought back when the account is read again, said plainly); later, an
  opt-in Apps Script per account (lot D): free, in the user's own
  account, but installed by hand in the browser with a Google warning
  ("application non validée"), and a Workspace may forbid it.
- **Q4. People's comments on watched repositories.** Proposed: counted
  only; a person's comment interrupts where the user takes part
  (author, commented, mentioned, review requested, assigned).
- **Q5. The bar's place and look.** Proposed: « COURRIER » under To do,
  one line per account with an amber count and the oldest one's age; the
  mini form one item, a line per account.
- **Q6. « Le jour J ».** Proposed: offered only when the mail says a date
  (a reservation's or an invitation's), read when the picker opens; back
  at 8:00 that day (the day before for a flight?).
- **Q7. Undo and limits.** Proposed: a 10 s toast, a 30-day journal, 100
  conversations a batch, no confirmation dialog (the undo instead).
- **Q8. A snooze comes back lower on the phone.** Gmail orders the inbox
  by the last message's date: a conversation brought back by labels is
  not on top, as Gmail's own snooze would be. Proposed: accept it (the
  bar and the bubble show it on top); starring or adding a message to
  lift it are rejected (writes the user did not ask for).

## 15. Room for what comes next

#656's other directions (a mail window with drafts and replies on hold,
search, a contact's history, the assistant preparing drafts) would use
the same crate: its accounts, conversations and journal. Sending, drafts
and reading bodies at length are new writes and new reads: each a
decision of its own, with its own spec and its own row in §10.2's table.

## 16. Tests (headless)

- **A fake Gmail IMAP server** (the new crate's
  `tests/fake_imap.rs`, on a local port, TLS with a test certificate): `CAPABILITY`,
  `AUTHENTICATE XOAUTH2` (checking the token the fake Online Accounts
  gave), `LIST` with special-use, `EXAMINE`, `SELECT`, `UID SEARCH`
  (`X-GM-RAW` for the few searches used), `UID FETCH` (headers, flags,
  `X-GM-*`), `UID STORE`, `CREATE`, `DELETE`, `IDLE`; a mailbox of
  messages with labels. **It refuses and records every other command**
  (`EXPUNGE`, `MOVE`, `COPY`, `APPEND`, `\Deleted`, a label not the
  lens's, `DELETE` of a label that holds mail); every test ends by
  asserting none came. It changes the mailbox "from elsewhere" (the
  phone) and says so over IDLE. `HOCUS_TEST_IMAP` names it, taken only
  on this machine, as `HOCUS_TEST_GOOGLE_API` is.
- **Fixtures** (the crate's `fixtures/*.eml`, made by hand, no real
  mail): a person; a list; a newsletter; GitHub's review request,
  mention, team mention, assign, security alert, a failed run on main
  and on a branch, a passing run, a bot's comment, a person's comment,
  a subscribed mail; GitLab's review request, mention, a failed
  pipeline, a mail without its reason header; a hotel's reservation with
  JSON-LD; an invitation with its `text/calendar`; broken headers.
- **Unit**: each fixture's verdict and reason (one table); a person
  override; `people = "known"`; « Le jour J »'s date; the labels'
  names read and written; the picker's choices on a Friday evening; the
  writes' type building exactly §10.2's commands.
- **The crate against the fake** (its `tests/`): one test
  a scenario of §13 that has no screen: 2 (IDLE), 7, 8, 9, 10 (the
  commands and the mailbox after the undo), 12 (`HOCUS_CLOCK_AT` past
  the label's time as the worker starts), 13, 14 and 15 (the fake
  refusing the token with Google's answers), 16, 19 to 25; with a
  read-only account, nothing but `EXAMINE` and `FETCH`.
- **The shell** (`crates/hocus-compositor/tests/mail.rs`, the real
  shell, the fake Online Accounts of `calendar.rs` on the fakes'
  private bus, the fake server): the bar's line from calm to « 1 », the
  bubble and its reason, « Lu et archivé » from the popover and the
  line going back to calm, the toast's « Annuler »; the Settings page's
  rows written (`settings_mail.rs`). What the bar and the popover look
  like is the maintainer's, on screenshots.

## 17. Lots

- **A. Reading** (the crate can only `EXAMINE` in this lot): accounts
  and consent, IMAP and IDLE, the rules, the bar's group and mini item,
  the popover's lists with « Pourquoi ? » and « Ouvrir dans Gmail »,
  bubbles, observing, Settings › Courrier. Scenarios 1–6, 11, 13–18, 22,
  23.
- **B. Lu et archivé**: one and a batch, the toast, the journal, undo.
  Scenarios 10, 19–21.
- **C. Plus tard**: the picker, the labels, bringing back, « Dans 2
  min », « Le jour J ». Scenarios 7–9, 12, 24, 25.
- **D. Back with the computer off** (if Q3 says so): the script per
  account, its install sheet.
