require "json"

VERSION = "1.0"

module Billing
  TAX_RATE = 0.2

  class Invoice < Base
    attr_accessor :name
    attr_reader :total, :lines

    def initialize(name)
      local = name.strip
      @name = local
    end

    def self.build(attrs)
      new(attrs[:name])
    end

    def paid?
      false
    end

    class << self
      def cached
        @cached ||= {}
      end
    end

    alias label name
  end

  module Helpers
    def self.format(amount) = amount.to_s
  end
end

Billing::DEFAULT_RATE = 0.1

class Billing::Report
  def render; end
end

def top_level(arg)
  temp = arg
  temp
end
