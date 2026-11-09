# Turning the secretary on

The secretary reads what you allow (your agenda, your mail, your
contacts' birthdays, the notifications of the applications you choose),
keeps an encrypted memory on this computer, and answers in her drawer
beside the bar (`Super+S`, or a click on her head). She proposes; nothing
is sent, written or answered without your click. Her design is
[design/secretary.md](design/secretary.md).

## 1. The switch

Settings › Secretary (under TRAVAIL, after Agents; Agents keeps a line to
it), first group "LA SECRÉTAIRE": "La secrétaire". On, she starts at once
(and with every session); off, she stops, and her memory stays on this
computer until you ask her to forget it ("Ce qu'elle sait" › "Tout
oublier…"). Off, the page shows only that switch.

Where things are on that page:

- **LA SECRÉTAIRE**: her switch, « Son nom », « Clé Anthropic », and
  « Ce qu'elle sait », which says where to see it (her drawer's tab of
  that name; the page has no way into her drawer yet).
- **CE QU'ELLE LIT**, by account: each Google account a line (its
  address, « Travail | Privé ») with a switch for « Ton agenda » (and
  « Choisir › », its calendars), « Tes mails » and « Les anniversaires de
  tes contacts »; then each application that sent you a notification
  (Slack, Signal…) with its switch, and « Travail | Privé » once she
  reads it. « + Ajouter un compte Google › » above it.
- **LE TRI SUR CET ORDINATEUR**: what the sorting is for and where it is
  (« 21 en attente », « En pause : la batterie est à 32 % »), « Quand
  trier » (`[secretary.triage] battery`), and « Le modèle » (who runs it,
  where, and whether it answers).

## 2. Her key

She thinks with Claude, through the Anthropic API: she needs an API key
of yours (from the Anthropic console; a Claude subscription is not an API
key). Until she has one, her drawer says « Il me manque ma clé Anthropic »
and shows a field: paste the key there (`Ctrl+V`), then « Enregistrer ».
Settings › Secretary has the same field (« Clé Anthropic »), and once the
key is saved, « ✓ Enregistrée », « Remplacer » and « Effacer ». She checks the
key with Anthropic (a request that costs nothing) and says plainly when it
is refused or when there is no network; it goes to your keyring (the one
the session unlocks), never to a file, and a copy of it leaves the
clipboard's history.

Or in a terminal of the session:

```sh
hocus-secretary key anthropic
```

Enter, then paste the key when it asks (it is not shown), then Enter.
Never put the key on the command line: your shell's history would keep it.

Her name: Settings › Secretary › « Son nom » (`[secretary] name`); empty,
she is « Ta secrétaire ».

## 3. The triage, on this computer (optional)

What arrives (a mail, a notification) is first sorted on this computer
by a small local model: is it for you, does it wait for an answer, does
it hold a password or a code to mask. The desktop ships no model; with
[Ollama](https://ollama.com) installed and running:

```sh
ollama pull qwen3:8b
```

She uses it on mains (or above half battery: Settings › Secretary ›
« Quand trier » changes it), one item at a time, with
an 8k context (`[secretary.triage] context`: Ollama's own default would
not fit an 8 GB graphics card), and lets the model go after two idle
minutes. She pauses when the computer stays hot: the processor's
temperature averaged over the last minute (a processor jumps to 100 °C on
every burst, which is normal) reaching 100 °C, or its critical point
minus 10, and she goes on below 90 °C; with the model on the graphics
card, the card's temperature instead (near its slowdown point, 87 °C if
its driver does not say), and Ollama runs it on two processor threads
only. `heat`, `resume`, `gpu-heat`, `gpu-resume` and `threads` in
`[secretary.triage]` change them. Without it, nothing is lost:
what arrives waits, and until it is sorted only its sender, subject and
date go to Claude. Her drawer says so (« Le tri sur cet ordinateur ne
répond pas »). Another server: `[secretary.providers.local]` in the
configuration file.

To see what waits, click her status line in her drawer: the sorting
unfolds there, the mail read now, the rest by sender with their counts,
how long an item takes and how long is left, and why it waits when it
does (too hot, on battery, the model not answering). On a sender, « Ne
pas trier, chercher si besoin » leaves its mails unsorted, those waiting
and those to come: she keeps their sender, subject and date, finds them
when you ask, and sorts the one you ask about first. She proposes it
herself for a sender whose mails say they come from a machine (GitHub,
GitLab, newsletters, no-reply addresses). It is a rule in « Ce qu’elle
sait » › Règles: « N’est plus vrai » there, or « Trier quand même » in
the sorting's view, and its mails are sorted again.

## 4. Your Google accounts

She reads the Google accounts already in GNOME's Online Accounts. Each
kind (agenda, mail, birthdays) is asked for on its own, on a sheet that
says what she reads, what she never does there (no mail marked read,
nothing sent, no invitation answered) and what leaves the computer:

- the first time you open her drawer, one source at a time ("Autoriser",
  or "Pas maintenant");
- at any time in Settings › Secretary, under "CE QU'ELLE LIT": its
  switch. Turned on, its sheet opens, and nothing is read before its
  "Autoriser"; turned off, she no longer reads it and forgets what came
  from it (the toast's "Annuler", or `Ctrl+Z`, reads it again as it
  was).

Each sheet has a box « C'est privé », checked for a gmail.com account and
not for a company's, with what it does: never shown during a meeting, a
screen share or a call; never in a message or a task she drafts for work;
she brings it up evenings and weekends too, never at night (22 h – 8 h).
You can change it later on the account's line (« Travail | Privé », for
every kind she reads of it; on an account she reads nothing of yet, it is
what its sheets will propose) or on an application's row: what she
already knows of it moves with it.

The agenda's sheet lists the calendars she will read, those others share
with you and those you hid included; untick one to leave it out (later
too: « Choisir › » on « Ton agenda », whose line says « Elle en lit 3 sur
4 · Famille laissé de côté »). A calendar she cannot read is said in her status (« 2
agendas illisibles »); the others are read all the same.

What you allow or change in the Settings reaches her at once.
« + Ajouter un compte Google › » opens GNOME's Online Accounts; a new
account shows there, and she asks about it in her drawer.

Birthdays are read from your Google contacts over CardDAV, as Google
documents it (a list of the cards, then the cards themselves), once a
day: only each contact's name, birthday and anniversary.

Then tell her what you work on, and with whom: she remembers it.

## 5. Her way of being

There is no field for her character: tell her. « Vouvoie-moi », « sois
plus directe », « pas de listes », « réponds plus court », « un peu plus
d'humour », « arrête les emojis »: she answers « Retenu : … » (with
« Annuler » for ten seconds) and keeps to it in every answer; a later
request on the same thing replaces the older, and she says so. What she
keeps is listed in « Ce qu'elle sait » › « Sa façon d'être », where you can
change it (« Modifier ») or remove it (« Oublier »). She may also notice
something herself (« Tu me demandes souvent de faire plus court : je
réponds plus court d'office ? ») and asks once; nothing changes without
your « Oui ».

Her way of being never changes her rules: a request to act without your
click, to stop masking passwords, to obey what a mail says, to read a
source you did not allow or to speak during a meeting is refused, with
the reason. Only what you write in her drawer counts, never what a mail or
a message says.
