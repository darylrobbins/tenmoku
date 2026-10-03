// A small sliding-window counter, used as a golden highlighting sample.
using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

namespace Golden.Metrics;

/// <summary>Counts events over the last <c>Window</c> of time.</summary>
public sealed class WindowCounter<TKey> where TKey : notnull
{
    private const int DefaultCapacity = 64;
    private readonly Dictionary<TKey, Queue<DateTime>> _events = new();
    private static int s_instances;

    public TimeSpan Window { get; init; } = TimeSpan.FromSeconds(30);
    public int Count => _events.Values.Sum(q => q.Count);

    public event EventHandler<TKey>? Overflowed;

    public WindowCounter(int capacity = DefaultCapacity)
    {
        Capacity = capacity > 0 ? capacity : throw new ArgumentOutOfRangeException(nameof(capacity));
        s_instances++;
    }

    public int Capacity { get; }

    public bool Record(TKey key, DateTime? at = null)
    {
        var now = at ?? DateTime.UtcNow;
        if (!_events.TryGetValue(key, out var queue))
        {
            queue = new Queue<DateTime>(Capacity);
            _events[key] = queue;
        }

        Trim(queue, now);
        if (queue.Count >= Capacity)
        {
            Overflowed?.Invoke(this, key);
            return false;
        }
        queue.Enqueue(now);
        return true;
    }

    private void Trim(Queue<DateTime> queue, DateTime now)
    {
        while (queue.TryPeek(out var oldest) && now - oldest > Window)
        {
            queue.Dequeue();
        }
    }

    public IEnumerable<(TKey Key, int Hits)> Top(int n) =>
        from pair in _events
        let hits = pair.Value.Count
        orderby hits descending
        select (pair.Key, hits);

    public async Task<string> DescribeAsync()
    {
        await Task.Yield();
        string label = Count switch
        {
            0 => "idle",
            < 10 => "quiet",
            _ => $"busy ({Count} events, {s_instances} counters)",
        };
        return @"window: " + label + '\n';
    }
}

[Flags]
public enum Level : byte
{
    None = 0,
    Warn = 1 << 0,
    Error = 1 << 1,
}

public record struct Sample(string Name, double Value = 0.5);
