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
key). In a terminal of the session:

```sh
hocus-secretary key anthropic
```

Paste the key, then Enter. It goes to your keyring (the one the session
unlocks), never to a file; she reads it at her next answer. Until then,
her drawer says « Il me manque ma clé Anthropic » and how to give it.

## 3. The triage, on this computer (optional)

What arrives (a mail, a notification) is first sorted on this computer
by a small local model: is it for you, does it wait for an answer, does
it hold a password or a code to mask. The desktop ships no model; with
[Ollama](https://ollama.com) installed and running:

```sh
ollama pull qwen3:8b
```

She uses it on mains (or above half battery), one item at a time, and
lets the model go after two idle minutes. Without it, nothing is lost:
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

A company's account is suggested as work ("pro"), a gmail.com one as
personal ("perso"); you choose on the sheet. Personal items never show
during a meeting, a screen share or a call.

Then tell her what you work on, and with whom: she remembers it.
