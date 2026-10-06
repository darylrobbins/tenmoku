<?php

namespace App\Models;

const APP_VERSION = '1.0';

define('LEGACY', true);

$registry = [];
list($first, $second) = [1, 2];

interface HasName
{
    const PREFIX = 'n';

    public function name(): string;
}

trait Greets
{
    public function greet(): string
    {
        return 'hi';
    }
}

abstract class Model implements HasName
{
    use Greets;

    public const TABLE = 'models';
    protected string $name;
    private static int $count = 0, $other;

    public function __construct(private int $id)
    {
    }

    public function name(): string
    {
        $local = $this->name;
        return $local;
    }

    abstract protected function save(): void;
}

enum Status: string
{
    case Active = 'active';
    case Archived = 'archived';

    public function label(): string
    {
        return $this->value;
    }
}

function helper(int $param): int
{
    $temp = $param;
    return $temp;
}
