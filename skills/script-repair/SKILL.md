---
name: script-repair
tags: automation-script, rpa, repair, maintenance, desktop-automation, ai-agent
requirements:
  - agent-record
description: >
  Repair a script made from a recording (recording-to-script) that stopped working because the
  app or the web page changed: a renamed button, a moved or new field, a new dialog, a list with
  other options. Finds the first step that no longer matches, changes only that step, and runs
  the whole script again. Works on a script in any language.
  Triggers on: "the script failed", "el script falló", "repair the script", "repara el script",
  "it stopped at step 4", "Step 4 (...) failed", "the page changed", "it no longer works",
  "fix my automation".
---
<!-- Part of Agent Record Adapter (see NOTICE). -->

# Repairing a script

A script made from a recording replays the task as it was. Apps and pages change: a button is renamed, a field moves or is split in two, a new required field appears, a dialog shows up, a list gets other options. The script then stops at the step that no longer matches, with a message such as:

```
Step 4 (Type the phone) failed: No element with id "phone" in "Northwind Cloud · Create your account"
```

A repair finds the step that no longer matches, changes only that step and runs the whole script again. It is done once, with the person; the script itself does not learn while it runs. Keep the script's own language and style: repair it in the language it was written in.

**Desktop apps and web pages.** Desktop apps are driven with agent-record's desktop commands (`click`, `set-value`, `press --window-id`, `menu`, ...). A web page in a browser started with DevTools goes through the official agent-browser, run as `agent-record browser <agent-browser command>` (MCP: the browser tools of the same server); it also works in the background. The person's own browser without DevTools (where they are already signed in) is driven with the desktop commands (`navigate --window-id`, `click`), which reach its pages as window messages.

## 1. Find the first step that is wrong

The step that failed is not always the one that changed: a field that kept only part of a value makes the script fail later, at a step that was fine.

1. Run the script from a clean state (the app closed, the page not open) with the values that failed, and read its output: the last step it started and the error.
2. If it failed at a check, or reached the end with a wrong result, look at the steps before it: the first one whose check fails is where the repair starts. A step without a check should get one now: checks are what point at the real cause.

The error often says what changed:

| The error says | What it usually means |
|---|---|
| `No element with id "x"` / `No textfield "Name"` | The element was renamed, moved inside something new, or removed |
| `ELEMENT_NOT_FOUND ... no option ... (its options: ...)` | The list's options changed: the error lists the ones there are now |
| `AMBIGUOUS_TARGET` | Several elements now look the same |
| `does not hold what was written` / `ACTION_FAILED ... did not keep` | The field now changes or refuses the value (a new format, a mask) |
| `did not show "..."` after a click | The click did nothing visible: a new required field, an error message, or a new step in between |
| `POLICY_DENIED` | The step asked for something the tool does not do without permission (a dangerous shortcut, physical input not allowed): use the background alternative it names |

## 2. Bring the screen to that step

Run the script again from a clean state, but stop right before the wrong step: every step before it still works, so let the script do them rather than doing them by hand. A script made with `desktop-script.mjs` takes `--stop-before N` (and leaves its browser open); in another script, add such an option or comment out the steps after it while you repair.

Do not repeat an action that cannot be undone (send, post, delete, buy) to get there. If the failing step comes after one, use test data, or ask the person.

## 3. Look at what is there now

With the screen at the failing step, look before changing anything:

- `agent-record snapshot --window-id <id> -i --compact` (or `find` with a looser filter, such as `--text`) to see the elements there now, with their roles, names and ids.
- `agent-record screenshot --window-id <id> <file.png>` when the tree is not enough.
- The recording, and its video if there is one, to see what the step found when it was recorded.

Then name the cause and the change:

| What you see | What to change in the step |
|---|---|
| The element is there with another name | Find it by its id (`native_id`), which rarely changes, or by its new name |
| One field became two (a code list and a number) | Split the step: choose the code, write the rest. Derive both from the same parameter rather than asking the person for new ones |
| Several elements now match | Add the nearest named ancestor, or use the id |
| The element is inside a new tab, panel, menu or welcome screen | Add the step that opens it first |
| A new field is required | Add a step for it. Its value comes from the person: a new parameter with a question and an example, never an invented value |
| A field is gone | Remove its step, and its parameter if nothing else uses it |
| A dialog or banner now appears (cookies, a tip) | Add a step that answers it the way a person would (the least consent for cookies), only when it is there (`has` in `desktop-script.mjs`, a short `find` elsewhere) |
| The list has other options | Use the option the error lists that means the same; ask the person if none clearly does |
| The element appears later than before | Wait for it longer before acting |
| The page or app moved (new address, new program path) | Change the first steps that open it |
| The step works but the next screen is different | The task changed after this step: repair the next step the same way |

## 4. Change only that step

Change the smallest thing that makes the step work again: how it finds the element, a wait, one added or removed step. Keep everything else as it was, and keep the parameters and their names exactly: the person runs the script with them. If the repair needs a new value, ask the person for it and add it as a parameter. A new step between two others can take a number such as `4.5`, so the step numbers the person knows stay the same.

After the change, run the step on the screen you prepared and confirm what it left (the value in the field, the dialog that opened). Mark the change with a short comment that says what changed on the screen.

When the app may be in either version (some computers updated, some not), the step can try the new way and fall back to the old one:

```js
await step(4, 'Submit', async () => {
  const button = (await page.has({ role: 'button', name: 'Send' })) || (await page.find({ role: 'button', name: 'Submit' }));
  await button.click();
});
```

Never try two ways of a step that cannot be undone: if the first one acted, the second one would act again.

## 5. Run it all again

Run the whole script from a clean state with the values that failed. If it fails at a later step, repair that one the same way. The repair is finished when the script runs once from start to end and its final check passes.

Keep the previous version (a copy with the date, or version control) so the person can go back, and tell them:

- which steps changed, and what changed on the screen,
- any new parameter and its question,
- anything the script no longer does because the app no longer has it.

## When to record again instead

If most steps changed (a redesigned app, a new site), record the task again (`desktop-recording`) and build a new script from it (`recording-to-script`) instead of repairing many steps. You can also record only the part that changed and replace those steps.
