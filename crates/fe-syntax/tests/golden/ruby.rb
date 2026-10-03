# A tiny inventory ledger, used as a golden highlighting sample.
require "json"
require_relative "support/money"

module Ledger
  VERSION = "1.2.0"
  DEFAULT_LIMIT = 50

  class Error < StandardError; end

  # One stock movement: positive counts in, negative counts out.
  Entry = Struct.new(:sku, :count, :note, keyword_init: true) do
    def inbound?
      count.positive?
    end
  end

  class Book
    include Enumerable
    attr_reader :name, :entries

    def initialize(name, limit: DEFAULT_LIMIT)
      @name = name
      @limit = limit
      @entries = []
      @@books ||= 0
      @@books += 1
    end

    def record(sku, count, note = nil)
      raise Error, "unknown sku #{sku.inspect}" if sku.to_s.empty?
      raise ArgumentError, "count must be non-zero" if count.zero?

      @entries << Entry.new(sku: sku, count: count, note: note)
      @entries.shift while @entries.size > @limit
      self
    end

    def each(&block)
      return enum_for(:each) unless block_given?

      @entries.each(&block)
    end

    def balance(sku)
      select { |entry| entry.sku == sku }.sum(&:count)
    end

    def to_h
      {
        name: @name,
        skus: map(&:sku).uniq.sort,
        "total" => sum(&:count),
      }
    end

    def report(io = $stdout)
      group_by(&:sku).each do |sku, rows|
        io.printf("%-10s %5d\n", sku, rows.sum(&:count))
      end
    end

    def filter(pattern = /\A[A-Z]{3}-\d+\z/)
      reject { |e| e.sku !~ pattern }
    end

    private

    def to_s
      case @entries.length
      when 0 then "#{name}: empty"
      when 1..9 then "#{name}: #{@entries.length} entries"
      else "#{name}: busy"
      end
    end
  end
end

if __FILE__ == $PROGRAM_NAME
  book = Ledger::Book.new("main", limit: 3)
  book.record("ABC-1", 4).record("ABC-1", -1, "sold")
  puts JSON.pretty_generate(book.to_h)
  puts book.balance("ABC-1") > 0 ? 'in stock' : "out"
end
