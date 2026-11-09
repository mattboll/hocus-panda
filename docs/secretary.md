# Turning the secretary on

The secretary reads what you allow (your agenda, your mail, your
contacts' birthdays, the notifications of the applications you choose),
keeps an encrypted memory on this computer, and answers in her drawer
beside the bar (`Super+S`, or a click on her head). She proposes; nothing
is sent, written or answered without your click. Her design is
[design/secretary.md](design/secretary.md).

## 1. The switch

Settings › Agents, last group "LA SECRÉTAIRE": "La secrétaire
assistante". On, she starts at once (and with every session); off, she
stops, and her memory stays on this computer until you ask her to forget
it ("Ce qu'elle sait" › "Tout oublier…").

## 2. Her key

She thinks with Claude, through the Anthropic API: she needs an API key
of yours (from the Anthropic console; a Claude subscription is not an API
key). Until she has one, her drawer says « Il me manque ma clé Anthropic »
and shows a field: paste the key there (`Ctrl+V`), then « Enregistrer ».
Settings › Agents › LA SECRÉTAIRE has the same field (« Clé Anthropic »),
and once the key is saved, « Remplacer » and « Effacer ». She checks the
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

Her name: Settings › Agents › « Son nom » (`[secretary] name`); empty, she
is « Ta secrétaire ».

## 3. The triage, on this computer (optional)

What arrives (a mail, a notification) is first sorted on this computer
by a small local model: is it for you, does it wait for an answer, does
it hold a password or a code to mask. The desktop ships no model; with
[Ollama](https://ollama.com) installed and running:

```sh
ollama pull qwen3:8b
```

She uses it on mains (or above half battery), one item at a time, with
an 8k context (`[secretary.triage] context`: Ollama's own default would
not fit an 8 GB graphics card), and lets the model go after two idle
minutes. Without it, nothing is lost:
what arrives waits, and until it is sorted only its sender, subject and
date go to Claude. Her drawer says so (« Le tri sur cet ordinateur ne
répond pas »). Another server: `[secretary.providers.local]` in the
configuration file.

## 4. Your Google accounts

She reads the Google accounts already in GNOME's Online Accounts. Each
kind (agenda, mail, birthdays) is asked for on its own, on a sheet that
says what she reads, what she never does there (no mail marked read,
nothing sent, no invitation answered) and what leaves the computer:

- the first time you open her drawer, one source at a time ("Autoriser",
  or "Pas maintenant");
- at any time in Settings › Agents, under "LA SECRÉTAIRE": "Autoriser",
  or "Ne plus lire" (she then forgets what came from it).

Each sheet has a box « C'est privé », checked for a gmail.com account and
not for a company's, with what it does: never shown during a meeting, a
screen share or a call; never in a message or a task she drafts for work;
she brings it up evenings and weekends too, never at night (22 h – 8 h).
You can change it later on the source's row (« Privé »): what she already
knows of it moves with it.

The agenda's sheet lists the calendars she will read, those others share
with you and those you hid included; untick one to leave it out (later
too, on its row). A calendar she cannot read is said in her status (« 2
agendas illisibles »); the others are read all the same.

What you allow or change in the Settings reaches her at once. Under the
Google rows, « Ajouter un compte Google » opens GNOME's Online Accounts; a
new account shows there, and she asks about it in her drawer.

Then tell her what you work on, and with whom: she remembers it.
