# fe-chat-anthropic's messages: why a request to the API failed.

chat-anthropic-http = HTTP { $status }
chat-anthropic-http-body = HTTP { $status }: { $body }
chat-anthropic-event-unreadable = the stream sent an unreadable `{ $event }` event: { $error }
chat-anthropic-stream-error = the stream sent an error
chat-anthropic-header-name = header { $name }: { $error }
chat-anthropic-header-value = header { $name }'s value: { $error }
chat-anthropic-connection-broke = the connection broke: { $error }
chat-anthropic-closed-before-end = the connection closed before the reply ended
chat-anthropic-closed-before-start = the connection closed before the reply started
chat-anthropic-request-must-end-with-user = a request must end with the user's message
