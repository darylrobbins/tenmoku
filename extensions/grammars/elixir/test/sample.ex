#!/usr/bin/env elixir
# A small inventory server, touching most of the language.

defmodule Inventory.Item do
  @moduledoc """
  An item in stock, with a name and a count.
  """

  @enforce_keys [:name]
  defstruct [:name, count: 0, tags: []]

  @type t :: %__MODULE__{name: String.t(), count: non_neg_integer(), tags: [atom()]}

  @doc "Builds an item with a starting count."
  @spec new(String.t(), integer()) :: t()
  def new(name, count \\ 0) when is_binary(name) and count >= 0 do
    %__MODULE__{name: name, count: count}
  end

  defp label(%__MODULE__{name: name, count: 0}), do: "#{name} (sold out)"
  defp label(%__MODULE__{name: name, count: n}), do: "#{name} x#{n}"

  def describe(item), do: item |> label() |> String.upcase()
end

defmodule Inventory.Server do
  use GenServer

  alias Inventory.Item
  require Logger

  @max 1_000
  @mask 0xFF_FF
  @ratio 3.14e-2
  @initial %{apple: 3, pear: 0}

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, @initial, opts)
  end

  @impl true
  def init(stock), do: {:ok, stock}

  @impl true
  def handle_call({:add, name, n}, _from, stock) do
    case Map.fetch(stock, name) do
      {:ok, count} when count + n <= @max ->
        {:reply, :ok, Map.put(stock, name, count + n)}

      {:ok, _count} ->
        Logger.warning("too many #{name}\n\tlimit: #{@max}")
        {:reply, {:error, :full}, stock}

      :error ->
        {:reply, :ok, Map.put(stock, name, n)}
    end
  end

  def handle_call(:report, _from, stock) do
    lines =
      for {name, count} <- stock, count > 0, into: [] do
        Item.describe(Item.new(Atom.to_string(name), count))
      end

    {:reply, Enum.join(lines, "\n"), stock}
  end

  def parse(line) do
    with [name, amount] <- String.split(line, ","),
         {n, ""} <- Integer.parse(amount),
         true <- n > 0 do
      {:ok, {String.to_atom(name), n}}
    else
      _ -> {:error, ~s(bad line "#{line}")}
    end
  end

  def query(table) do
    ~SQL"""
    SELECT name, count FROM items WHERE count > 0
    """
    Regex.match?(~r/^\d+-[a-z]+$/i, table)
    fn x, _unused -> x * 2 end
  end
end

{:ok, pid} = Inventory.Server.start_link(name: :inventory)
GenServer.call(pid, {:add, :apple, 2})
IO.puts(GenServer.call(pid, :report))
