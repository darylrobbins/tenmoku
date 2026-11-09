#!/usr/bin/env rdmd
/**
 * A small inventory: structs, classes, templates and ranges.
 */
module inventory.app;

import std.stdio : writeln, writefln;
import std.algorithm;
import core.stdc.string : strlen;

/+ Nesting comments /+ can nest +/ like this. +/

enum Unit { piece, kilogram, litre }

enum maxItems = 1_000;
enum double taxRate = 0.13;

/// One line of the inventory.
struct Item
{
    string name;
    uint count = 0x2A;
    double price = 1.5e3;
    Unit unit = Unit.piece;

    this(string name, uint count) @safe pure nothrow
    {
        this.name = name;
        this.count = count;
    }

    double total() const @nogc => price * count;
}

interface Shelf
{
    void put(Item item);
}

class Store : Shelf
{
    private Item[] items;
    protected static immutable string banner = "Store\topen\n";

    override void put(Item item)
    {
        assert(items.length < maxItems, "shelf is full");
        items ~= item;
    }

    ~this() {}
}

/// Sums anything with a `total` member.
T sum(T, R)(R range) if (is(T : double))
{
    T acc = 0;
    foreach (ref element; range)
        acc += element.total();
    return acc;
}

template isItem(T)
{
    enum bool isItem = is(T == Item);
}

auto label(Item item) @trusted
{
    auto path = r"C:\stock\items.txt";
    auto raw = `back\quoted`;
    auto greeting = i"Hello $(item.name), you have $(item.count)";
    char sep = '\'';
    return item.name ~ sep ~ path ~ raw;
}

unittest
{
    auto apple = Item("apple", 3);
    assert(apple.total() == 4500.0);
}

/++
 + Entry point.
 +/
void main(string[] args)
{
    auto store = new Store();
    store.put(Item("pear", 7));
    bool ready = true;
    int[] counts = [1, 2, 3];
    // A lambda and a UFCS chain.
    auto doubled = counts.map!(n => n * 2).filter!(n => n > 2).array;

    switch (args.length)
    {
    case 0:
        break;
    default:
        writefln("%d args", args.length);
    }

    try
    {
        throw new Exception("oops");
    }
    catch (Exception e)
    {
        writeln(e.msg, null, ready);
    }
    finally
    {
        scope (exit) writeln(__FILE__, __LINE__);
    }
}
