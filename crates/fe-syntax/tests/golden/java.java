// A bounded event log, used as a golden highlighting sample.
package com.example.golden;

import java.time.Instant;
import java.util.ArrayDeque;
import java.util.Deque;
import java.util.List;
import java.util.Objects;
import java.util.function.Predicate;

/**
 * Keeps the most recent events, dropping the oldest when full.
 *
 * @param <E> the event type
 */
public final class EventLog<E extends EventLog.Event> implements Iterable<E> {

    public interface Event {
        Instant at();

        default boolean isError() {
            return false;
        }
    }

    public record Message(Instant at, String text, int level) implements Event {
        public Message {
            Objects.requireNonNull(text, "text");
        }

        @Override
        public boolean isError() {
            return level >= 40;
        }
    }

    private static final int DEFAULT_CAPACITY = 1_024;

    private final Deque<E> events = new ArrayDeque<>();
    private final int capacity;
    private volatile long dropped;

    public EventLog() {
        this(DEFAULT_CAPACITY);
    }

    public EventLog(int capacity) {
        if (capacity <= 0) {
            throw new IllegalArgumentException("capacity must be positive: " + capacity);
        }
        this.capacity = capacity;
    }

    public synchronized void add(E event) {
        while (events.size() >= capacity) {
            events.removeFirst();
            dropped++;
        }
        events.addLast(event);
    }

    public synchronized List<E> matching(Predicate<? super E> filter) {
        return events.stream().filter(filter).toList();
    }

    @Override
    public java.util.Iterator<E> iterator() {
        return List.copyOf(events).iterator();
    }

    public static void main(String[] args) throws Exception {
        var log = new EventLog<Message>(2);
        for (int i = 0; i < 3; i++) {
            log.add(new Message(Instant.now(), "event " + i, i * 20));
        }
        char marker = args.length > 0 ? args[0].charAt(0) : '*';
        double ratio = log.dropped / 3.0;
        boolean anyErrors = !log.matching(Message::isError).isEmpty();
        System.out.printf("%c dropped=%d ratio=%.2f errors=%b%n", marker, log.dropped, ratio, anyErrors);
        Object nothing = null;
        assert nothing == null : "unreachable";
    }
}
