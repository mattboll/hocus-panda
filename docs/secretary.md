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
key, and the key is billed by use, apart from it). Until she has one, her
drawer says « Il me manque ma clé Anthropic », explains what the key is,
has « Ouvrir la console d'Anthropic ↗ » (your browser opens on the page
where keys are made) and a field: paste the key there (`Ctrl+V`; it
starts with `sk-ant-`), then « Enregistrer ». A key refused is said under
the field, with what to do. Nothing else is asked before her key: her
first run's steps (« Pour commencer ») show where you are.
Settings › Secretary has the same field (« Clé Anthropic »), and once the
key is saved, « ✓ Enregistrée », « Remplacer » and « Effacer ». She checks the
key with Anthropic (a request that costs nothing) and says plainly when it
is refused or when there is no network; it goes to your keyring (the one
the session unlocks), never to a file, and a copy of it leaves the
clipboard's history.

Or, for scripts, in a terminal of the session (her drawer never shows this):

```sh
hocus-secretary key anthropic
```

Enter, then paste the key when it asks (it is not shown), then Enter.
Never put the key on the command line: your shell's history would keep it.

**Her memory's key** is another one: 32 random bytes she makes at her
first start to encrypt her memory, kept in the same keyring. When the
session has no keyring then (none installed, or one that does not
answer), she keeps that key in a file beside her memory
(`~/.local/share/hocuspanda/secretary/memory.key`, readable by you
alone), and says so where her memory is shown, in her drawer's « Ce
qu’elle sait » and in Settings › Secretary: « Sa mémoire est chiffrée,
mais sa clé est rangée à côté, sur ce disque : ton trousseau n’était pas
disponible quand elle a été créée. Qui peut lire tes fichiers peut donc
lire sa mémoire. » It means the encryption then only protects a copy of
the memory's file alone (a backup of `memory.db` without its key), not
the folder: whoever reads your home directory reads both. With a keyring,
the key is unlocked with your session and is in no file. A key file,
once made, stays her memory's key.

Both keys are overwritten with zeros in her process as soon as they have
been used (the one she asks Anthropic with is kept while she runs, and
wiped when it is replaced, erased, or she stops).

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
date go to Claude. Her drawer's status says so in a few words (« Tri en
pause : l'ordinateur chauffe »), and the sorting's view says why, once
(« L'ordinateur chauffe (97 °C). Je reprends quand il redescend sous
90 °C ; rien n'est perdu. »). Another server:
`[secretary.providers.local]` in the configuration file. The sorting
only ever goes to this computer: its address must be `http://localhost`,
`http://127.0.0.1` (any address in 127.0.0.0/8) or `http://[::1]`, with
or without a port. Its host is what is compared, so an address that only
begins like one (`http://127.0.0.1:x@elsewhere.example/`,
`http://localhost.elsewhere.example/`) is refused, as is any address
carrying a user or a password. Her other providers are called over
`https://`, or in clear on this computer only.

To see what waits, click « 21 mails à trier › » under her name in her
drawer (it shows while mails wait): the sorting unfolds there, under
« ‹ Retour à la discussion » and « Le tri de tes mails », with a sentence
saying what the sorting is for; the mail read now, how long is left, then
one card per sender: those sent by machines (notifications, newsletters),
the people, whom she sorts first. On a machine's card, « Ne pas trier les
mails de GitHub » (« Ne pas trier cette lettre » for a newsletter) leaves
its mails unsorted, those waiting and those to come: she keeps their
sender, subject and date, finds them when you ask, and sorts the one you
ask about first. A person's mails are never offered to be left unsorted:
you would miss that they wait for an answer. She proposes it herself for
a sender whose mails come from a machine (« GitHub t'a envoyé 11
notifications automatiques. Je les laisse sans tri ? »). It is a rule in
« Ce qu’elle sait » › Règles: « Ce n’est plus vrai » in its « ··· » menu
there, or « Trier à nouveau les mails de GitHub » under « Pas triés, à ta
demande » in the sorting's view, and its mails are sorted again.

## 4. Your Google accounts

She reads the Google accounts already signed in on this computer (GNOME's
Online Accounts). Each kind (agenda, mail, birthdays) is a consent of its
own, with what she reads, what she never does there (no mail marked read,
nothing sent, no invitation answered) and what leaves the computer:

- the first time you open her drawer, one card per account: a switch per
  kind (the agenda and the mails on, the birthdays off), « Ce compte est
  Travail | Privé », what she reads exactly folded under « Ce que je lis
  exactement, et ce qui part chez Anthropic › », then « Autoriser l'agenda
  et les mails » (it says what the switches allow) or « Pas maintenant »;
  the next account shows dashed under it;
- at any time in Settings › Secretary, under "CE QU'ELLE LIT": its
  switch. Turned on, its sheet opens, and nothing is read before its
  "Autoriser"; turned off, she no longer reads it and forgets what came
  from it (the toast's "Annuler", or `Ctrl+Z`, reads it again as it
  was).

« Privé » is proposed for a gmail.com account, « Travail » for a
company's; « Privé » means: never shown during a meeting, a
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

Then tell her what you work on, and with whom (her first run's third
step): she remembers it.

## Her drawer

Her name, and her state in a few words (« Tout est à jour », « Je ne lis
encore rien », « Tri en pause : l'ordinateur chauffe »). On a day nothing
is said yet, she says hello, what she reads, and three things to ask her.
What she proposes says where it leaves and to whom (« Message Slack · à
Léa Martin, en privé ») with « Envoyer sur Slack », « Modifier » and « Ne
pas envoyer »; nothing leaves before that click, and the card then says
what left, where and when (« ✓ Envoyé sur Slack à 9 h 12 »). Her questions
are labelled « Question », in blue; what she notes says « Noté : … » with
« Annuler » and a bar emptying over its ten seconds. Under the field, only
how to send: her model's name is in the Settings.

Her field is edited as any: the arrows move the caret (with Ctrl, by
word), Home and End go to the line's ends, Up and Down between lines,
Shift with any of them selects, `Ctrl+A` selects it all; a click places
the caret, a drag selects, a double click takes a word, a triple click the
line, Shift+click extends; Backspace and Delete (with Ctrl, a word),
`Ctrl+C`, `Ctrl+X`, `Ctrl+V`, `Ctrl+Z` to undo and `Ctrl+Shift+Z` to redo.
What you type replaces what is selected. Enter sends, Shift+Enter goes to
the line; `↑` in an empty field brings back your last sentence. An input
method's text shows underlined at the caret while you compose it. Her
key's field edits the same way, masked, and nothing copies it out.

Her text is selected with the pointer as in any chat: her answers, your
sentences, her questions, the drafts she proposes, her morning briefing
(its card and the briefing unfolded) and the lines of « Ce qu'elle sait »
(a click on a line there still unfolds where she has it from). Drag over it (across
several messages if you like; held above or below the conversation, it
scrolls), double-click a word, triple-click a paragraph, then press
`Ctrl+C`, or right-click and choose « Copier ». It copies as plain text, a
line between paragraphs and between messages, without the asterisks of
her bold or italic words; a secret she masked copies as its mask. A click
elsewhere or `Esc` clears the selection.

« Ce qu'elle sait »: what is to confirm first (« C'est juste · Faux » for
what she guessed, « Oui · Non » for what she proposes; she uses none of it
until you answer), what she learned this week, then the rest; each line
has « ··· » on hover (Modifier, Ce n'est plus vrai, Voir d'où je le sais,
Oublier) and says plainly where she has it from (« Deviné d'après le
ticket « Refonte de l'accueil » », « Vu dans Slack, le 9 nov. »).
« Travail et privé · Travail · Privé » beside the search; the chips (none
chosen shows everything; « Ma façon d'être » has its own). « Exporter »;
« Effacer… ▾ » apart (« Oublier tout le privé… », « Tout oublier… »),
always confirmed.

Her morning briefing comes out of the bar with what is in it (« Ton point
du matin est prêt. 3 réunions, 2 mails attendent ta réponse »), « Voir le
point » and « Plus tard ▾ ». The briefing names your free slot, and each
mail waiting for your answer has « Ouvrir le mail ↗ » (in your browser;
she never marks it read) and « Rappelle-le-moi » (two hours later).

## 5. Her way of being

There is no field for her character: tell her. « Vouvoie-moi », « sois
plus directe », « pas de listes », « réponds plus court », « un peu plus
d'humour », « arrête les emojis »: she answers « Noté : … » (with
« Annuler » for ten seconds) and keeps to it in every answer; a later
request on the same thing replaces the older, and she says so. What she
keeps is listed in « Ce qu'elle sait » › « Ma façon d'être », where you can
change it or remove it from its « ··· » menu (« Modifier », « Oublier »). She may also notice
something herself (« Tu me demandes souvent de faire plus court : je
réponds plus court d'office ? ») and asks once; nothing changes without
your « Oui ».

Her way of being never changes her rules: a request to act without your
click, to stop masking passwords, to obey what a mail says, to read a
source you did not allow or to speak during a meeting is refused, with
the reason. Only what you write in her drawer counts, never what a mail or
a message says.

## 6. Your day, and what you tell her

She sees your whole agenda for today and tomorrow: every event of every
calendar she reads (yours, those shared with you), with its times, the
same meeting on several calendars once, and nothing you declined. When a
very full day (a dozen colleagues' calendars, rooms) does not fit what
she is given, your own events come first (those of your calendars, and
the invitations you accepted), and she knows what was left out. For
another day (« et jeudi ? »), or what was left out, she looks at the
agenda itself, read-only.

What you tell her of your work and your life (« hocus-panda est un
projet perso », « Léa Martin est ma voisine », « Atlas est un client »)
she keeps at once, in the notebook it belongs to (Travail or Privé):
« Noté : … » with « Annuler » for ten seconds, then it is in « Ce qu'elle
sait » with « tu me l'as dit » and its date, where you change it or
forget it. Only your own words in her drawer are kept this way, never what
a mail says.

What she reads in your mails about people and projects she knows becomes
proposals; someone she does not know whom a mail links to something she
knows (« Léa Martin travaille sur hocus-panda ») is a question (« Je
l'ajoute à ce que je sais ? »): nothing is kept before your « Oui ». She
never invents a setting or a tool: when something is missing, she says
so.
