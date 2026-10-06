using System;

namespace Shop.Orders
{
    public interface IOrder
    {
        decimal Total { get; }
        void Submit();
    }

    public class Order : IOrder
    {
        public const int MaxLines = 50;
        private readonly int id, version;
        public decimal Total { get; private set; }
        public event EventHandler Changed;

        public Order(int id)
        {
            var local = id * 2;
            this.id = local;
        }

        ~Order() { }

        public void Submit() { }

        public static Order operator +(Order a, Order b) => a;

        public int this[int index] => index;

        public class Line
        {
            public string Sku { get; set; }
        }
    }

    public struct Money
    {
        public decimal Amount;

        public override string ToString() => Amount.ToString();
    }

    public enum Status
    {
        Open,
        Closed = 2,
    }

    public record Receipt(int Number)
    {
        public string Note { get; init; }
    }

    public delegate void Notify(string message);
}
