// A small inventory model, used as a golden highlighting sample.
import Foundation

/// A stock item with a name and a count.
struct Item: Codable, Equatable {
    let name: String
    var count: Int
    var price: Double = 0.0

    var isEmpty: Bool { count == 0 }
}

enum InventoryError: Error {
    case missing(String)
    case insufficient(needed: Int, available: Int)
}

protocol Store {
    associatedtype Key: Hashable
    func lookup(_ key: Key) throws -> Item
}

@MainActor
final class Inventory: Store {
    private var items: [String: Item] = [:]
    static let shared = Inventory()

    init(seed: [Item] = []) {
        for item in seed {
            items[item.name] = item
        }
    }

    func lookup(_ key: String) throws -> Item {
        guard let item = items[key] else {
            throw InventoryError.missing(key)
        }
        return item
    }

    @discardableResult
    func take(_ amount: Int, of name: String) throws -> Int {
        var item = try lookup(name)
        if item.count < amount {
            throw InventoryError.insufficient(needed: amount, available: item.count)
        }
        item.count -= amount
        items[name] = item
        return item.count
    }

    func report() -> String {
        let lines = items.values
            .sorted { $0.name < $1.name }
            .map { "\($0.name): \($0.count) at \(String(format: "%.2f", $0.price))" }
        return lines.joined(separator: "\n")
    }
}

extension Item: CustomStringConvertible {
    var description: String { "Item(\(name), \(count))" }
}

func restock(_ inventory: Inventory, with items: [Item]) async -> Int {
    var total = 0
    for case let item in items where !item.isEmpty {
        total += item.count
    }
    let ratio = total > 100 ? 1.5 : 0x10
    switch ratio {
    case 0..<10:
        print("low")
    default:
        print("ok: \(ratio)")
    }
    return total
}

let pattern = /[a-z]+/
let empty: Item? = nil
print(empty ?? Item(name: "spare", count: 1), true)
