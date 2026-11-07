#!/usr/bin/env groovy
package com.example.demo

import groovy.transform.CompileStatic
import java.util.concurrent.*

/**
 * A small inventory model.
 * @param name the item name
 */
@CompileStatic
class Item {
    static final int MAX_STOCK = 1_000
    String name
    int count = 0
    double price = 9.99

    void restock(int extra) {
        this.count = this.count + extra
    }

    boolean inStock() {
        return count > 0
    }

    String toString() {
        "Item(${name}, count=$count)"
    }
}

interface Shop {
    void add(Item item)
}

def square(int x) {
    return x * x
}

// Collections and closures
def items = [new Item(name: 'apple', count: 3), new Item(name: "pear\n")]
def names = items.collect { it.name.toUpperCase() }
def lookup = [a: 1, b: 2, c: 0x1F]
def total = 0L

for (int i = 0; i < 10; i++) {
    if (i % 2 == 0 && i != 4) {
        total += square(i)
    } else if (i > 7) {
        break
    } else {
        continue
    }
}

while (total > 100) {
    total -= 50
}

switch (total) {
    case 0:
        println 'zero'
        break
    default:
        println "total is ${total}"
}

try {
    assert total >= 0
    def ratio = 3.14e2 / 2
    println(ratio ?: 1.5f)
} catch (Exception e) {
    println "failed: ${e.message}"
} finally {
    println 'done'
}

/* Safe navigation and ranges */
def first = items?.find { it.inStock() }
(1..5).each { n -> print n }
def flag = true && !false
def nothing = null
List<String> words = ['x', 'y']
Map<String, Integer> counts = [:]
