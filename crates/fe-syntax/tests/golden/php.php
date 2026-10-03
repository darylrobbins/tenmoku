<?php
// A small task queue, used as a golden highlighting sample.

declare(strict_types=1);

namespace App\Queue;

use InvalidArgumentException;
use function array_map;
use const PHP_INT_MAX;

interface Handler
{
    public function handle(Task $task): bool;
}

enum Priority: int
{
    case Low = 0;
    case High = 10;
}

/**
 * One unit of work.
 */
final class Task
{
    public const MAX_ATTEMPTS = 3;

    public function __construct(
        public readonly string $name,
        private Priority $priority = Priority::Low,
        protected int $attempts = 0,
    ) {
        if ($name === '') {
            throw new InvalidArgumentException("empty task name");
        }
    }

    public function retry(): static
    {
        $this->attempts++;
        return $this;
    }

    public function exhausted(): bool
    {
        return $this->attempts >= self::MAX_ATTEMPTS;
    }
}

abstract class Queue implements \Countable
{
    /** @var list<Task> */
    protected array $tasks = [];
    private static ?Queue $default = null;

    abstract protected function order(Task $a, Task $b): int;

    public function push(Task ...$tasks): void
    {
        foreach ($tasks as $task) {
            $this->tasks[] = $task;
        }
        usort($this->tasks, fn (Task $a, Task $b) => $this->order($a, $b));
    }

    public function drain(Handler $handler, int $limit = PHP_INT_MAX): array
    {
        $failed = [];
        while ($limit-- > 0 && ($task = array_shift($this->tasks)) !== null) {
            if (!$handler->handle($task) && !$task->retry()->exhausted()) {
                $failed[] = $task;
            }
        }
        return array_map(static fn (Task $t): string => $t->name, $failed);
    }

    public function count(): int
    {
        return count($this->tasks);
    }
}

$names = ['a' => 1, "b" => 2.5, 'c' => true];
echo <<<TEXT
    Queued {$names['a']} task(s) at 0x1F
    TEXT;
?>
<p>Done.</p>
