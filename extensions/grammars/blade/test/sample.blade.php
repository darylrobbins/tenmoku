{{-- A Laravel view: the customer's order list. --}}
@extends('layouts.app')

@section('title', 'Orders')

@push('styles')
    <style>
        .badge { color: #c0392b; padding: 2px 4px; }
    </style>
@endpush

@section('content')
    @php
        $limit = 10;
        $title = "Orders for {$customer->name}\n";
        $tax = $customer->taxRate ?? 0.2;
        if (! isset($t)) {
            $t = $customer instanceof \App\Models\Guest ? 'warning' : 'info';
        }
    @endphp

    <!-- Plain HTML comments stay comments. -->
    <h1 class="title">{{ $title }}</h1>
    <p>Total: {{ number_format($total * (1 + $tax), 2) }} &euro;</p>
    <div class="notice">{!! $customer->bio !!}</div>

    <x-alert :type="$t" title="Heads up" />
    <x-alert
        ::class="{ 'shadow-lg': open }"
        @if($customer->vip)
            :type="$customer->tier"
            ::style="{ opacity: 0.9 }"
        @endif
    />

    <form method="POST" action="{{ route('orders.store') }}">
        @csrf
        <input type="text" name="note" value="{{ old('note', 'none') }}">
        <button type="submit" wire:click="save(42)">Save</button>
    </form>

    <div x-data="{ open: false, count: 3 }">
        <button @click="open = !open">Toggle</button>
        <span @if($compact) :class="open ? 'is-open' : ''" @endif>{{ $orders->count() }}</span>
        <ul x-show="open">
            @foreach ($orders as $index => $order)
                @if ($order->amount > 1000)
                    <li class="badge">{{ $index + 1 }}. {{ $order->name }}</li>
                @elseif ($order->status === 'void')
                    <li><s>{{ $order->name }}</s></li>
                @else
                    <li>{{ strtoupper($order->name) }}</li>
                @endif
            @endforeach
        </ul>
    </div>

    @isset($coupon)
        <p>Coupon: {{ $coupon->code }}</p>
    @endisset

    @forelse ($notes as $note)
        <p>{{ $note }}</p>
    @empty
        <p>No notes.</p>
    @endforelse

    @auth
        <a href="{{ url('/account') }}">Account</a>
    @endauth
@endsection

@push('scripts')
    <script>
        // Refresh the list every 30 seconds.
        const delay = 30 * 1000;
        setInterval(() => window.location.reload(), delay);
    </script>
@endpush
