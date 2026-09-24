---
name: recipe-review-meet-participants
description: "Review who attended a Google Meet conference and for how long."
metadata:
  version: 0.22.5
  openclaw:
    category: "recipe"
    domain: "productivity"
    requires:
      bins:
        - gwsr
      skills:
        - gwsr-meet
---
<!-- gwsr generated skill: do not edit by hand -->

# Review Google Meet Attendance

> **PREREQUISITE:** Load the following skills to execute this recipe: `gwsr-meet`

Review who attended a Google Meet conference and for how long.

## Steps

1. List recent conferences: `gwsr meet conferenceRecords list --format table`
2. List participants: `gwsr meet conferenceRecords participants list --params '{"parent": "conferenceRecords/CONFERENCE_ID"}' --format table`
3. Get session details: `gwsr meet conferenceRecords participants participantSessions list --params '{"parent": "conferenceRecords/CONFERENCE_ID/participants/PARTICIPANT_ID"}' --format table`

