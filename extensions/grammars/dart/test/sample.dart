#!/usr/bin/env dart
// A small inventory model.
library inventory;

import 'dart:async';
import 'dart:math' as math;

/// A kind of product.
enum Category { tool, toy, food }

/* Block comment
   over two lines. */
abstract class Item implements Comparable<Item> {
  final String name;
  final double price;
  static const int maxStock = 0xFF;

  Item(this.name, this.price);

  factory Item.free(String name) => _Gift(name);

  String get label => '$name costs ${price.toStringAsFixed(2)}';

  @override
  int compareTo(Item other) => price.compareTo(other.price);

  Future<void> restock({required int count, int delay = 10}) async {
    await Future.delayed(Duration(milliseconds: delay));
    for (var i = 0; i < count; i++) {
      if (i.isEven && count > 1e3) continue;
      print('Restocked #$i\tof "$name"\n');
    }
  }
}

class _Gift extends Item {
  _Gift(String name) : super(name, 0.0);
}

mixin Logger on Item {
  void log(String message) => print('[$name] $message');
}

class Shelf<T extends Item> {
  final List<T> items = [];
  final Map<String, int> counts = {};

  void add(T item) {
    items.add(item);
    counts.update(item.name, (n) => n + 1, ifAbsent: () => 1);
  }

  T? find(String name) {
    try {
      return items.firstWhere((item) => item.name == name);
    } on StateError catch (e) {
      print('missing: $e');
      return null;
    } finally {
      print('searched');
    }
  }
}

String describe(Object? value) {
  return switch (value) {
    int n when n > 10 => 'big',
    int() => 'small',
    null => 'nothing',
    _ => 'other',
  };
}

void main(List<String> args) {
  final shelf = Shelf<Item>();
  var total = 0.5 + 2 * math.pi;
  shelf
    ..add(Item.free('gift'))
    ..add(Item.free('card'));
  print(describe(42));
  print(r'raw \n string');
  print(true && !false ? total : 1_000);
}
