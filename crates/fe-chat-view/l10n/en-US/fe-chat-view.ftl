# The AI chat view's strings (crates/fe-chat-view).

## Code block states
chat-view-block-proposed = Proposed
chat-view-block-proposed-pending =
    { $pending ->
        [one] Proposed · { $pending } left
       *[other] Proposed · { $pending } left
    }
chat-view-block-accepted = Accepted
chat-view-block-rejected = Rejected
chat-view-block-accepted-some =
    { $accepted ->
        [one] { $hunks ->
            [one] Accepted { $accepted } of { $hunks }
           *[other] Accepted { $accepted } of { $hunks }
        }
       *[other] { $hunks ->
            [one] Accepted { $accepted } of { $hunks }
           *[other] Accepted { $accepted } of { $hunks }
        }
    }
chat-view-block-no-changes = No changes
chat-view-block-not-applied = Not applied
chat-view-block-copy = Copy
chat-view-block-apply = Apply

## Banner actions
chat-view-action-set-key = Set API Key
chat-view-action-open-settings = Open Settings
chat-view-action-retry = Retry
chat-view-action-edit = Edit
chat-view-action-remove-attachments = Remove Attachments

## Context meter
chat-view-meter =
    { $thousands ->
        [one] { $thousands }k of 1M tokens
       *[other] { $thousands }k of 1M tokens
    }
chat-view-meter-long =
    { $thousands ->
        [one] { $thousands }k of 1M tokens · Long chat: start a new one for better answers
       *[other] { $thousands }k of 1M tokens · Long chat: start a new one for better answers
    }

## Banners
chat-view-banner-cut-off = Reply cut off at the token limit
chat-view-banner-declined-category = Claude declined this request ({ $category })
chat-view-banner-declined = Claude declined this request
chat-view-banner-auth = Your API key was rejected
chat-view-banner-not-found = Model `{ $model }` isn't available to this key
chat-view-banner-too-large = Too much context
chat-view-banner-interrupted = Interrupted: { $message }
chat-view-reply-ended-early = the reply ended early

## Transcript
chat-view-thinking-open = ▾ Thinking
chat-view-thinking-closed = ▸ Thinking
chat-view-answered-by = Answered by { $model }
chat-view-review-changes =
    { $files ->
        [one] Review { $files } change
       *[other] Review { $files } changes
    }

## Composer
chat-view-stop = Stop
chat-view-send = Send
chat-view-chip = { $label } · { $estimate }
chat-view-chip-reading = { $label } · reading…
chat-view-chip-truncated = { $label } · { $estimate } · truncated
chat-view-chip-dropped = { $label } · { $estimate } · won't fit

## First-use notice
chat-view-notice-privacy = What you send goes to Anthropic (api.anthropic.com) under your API key: your messages, what you attach to them, the chat's earlier turns and its instructions. Nothing is sent until you press Send.
chat-view-notice-fallback = When the model asked for is unavailable, another Claude model may answer; the reply says which.
chat-view-notice-storage = Chats are kept on this computer, per folder, until you delete them; in a folder you haven't trusted, only until the window closes.
chat-view-notice-dismiss = Got It
chat-view-no-key = Add your Anthropic API key

## Commands (keys derive from the ids)
command-chat-send = Send Message
command-chat-stop = Stop Reply
command-chat-retry = Retry
