Goal: answer-bug-probe
Run ID: 01KRC69K2GZ1HA51KK8276VB44
Pipeline progress: 5 of 6 stages completed

## Stage: yes_no
- Status: succeeded
- Handler: human

## Stage: confirmation
- Status: succeeded
- Handler: human

## Stage: multiple_choice
- Status: succeeded
- Handler: human

## Stage: multi_select
- Status: succeeded
- Handler: human

## Stage: freeform
- Status: succeeded
- Handler: human

## Current context
| Key | Value |
|-----|-------|
| human.gate.confirmation.answer | yes |
| human.gate.confirmation.label | [Y] Continue |
| human.gate.confirmation.question | Confirm that you want to continue into more structured questions. |
| human.gate.freeform.answer | "QA probe — testing the answer wire path" |
| human.gate.freeform.question | Add any final context, constraints, or nuance for the summary. |
| human.gate.label | "QA probe — testing the answer wire path" |
| human.gate.multi_select.answer | S, B |
| human.gate.multi_select.label | [S] Success criteria, [B] Blockers |
| human.gate.multi_select.question | Which supporting areas should the final summary emphasize? |
| human.gate.multiple_choice.answer | R |
| human.gate.multiple_choice.label | [R] Risks |
| human.gate.multiple_choice.question | Which theme should be the center of the final summary? |
| human.gate.selected | freeform |
| human.gate.text | "QA probe — testing the answer wire path" |
| human.gate.yes_no.answer | yes |
| human.gate.yes_no.label | [Y] Yes |
| human.gate.yes_no.question | Is this interview workflow easy to follow so far? |


Summarize the full human interview. Include each question and answer in order, then synthesize the user's priorities, constraints, and open questions. Use the human.gate.<node>.question and human.gate.<node>.answer context keys when present. Do not invent missing answers.