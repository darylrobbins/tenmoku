#!/usr/bin/perl
# A small inventory report: packages, subs, regexes and a heredoc.
use strict;
use warnings;
use List::Util qw(sum max);
use constant PI => 3.14159;

package Inventory::Item;

our $VERSION = '1.02';

sub new {
    my ($class, %args) = @_;
    my $self = {
        name  => $args{name} // 'unnamed',
        count => $args{count} || 0,
        price => $args{price} // 0.0,
    };
    return bless $self, $class;
}

sub total {
    my $self = shift;
    return $self->{count} * $self->{price};
}

package main;

my @items = (
    Inventory::Item->new(name => "widget", count => 12, price => 2.50),
    Inventory::Item->new(name => "gadget", count => 0x1F, price => 1_000),
    Inventory::Item->new(name => 'gizmo', count => 0b101, price => 7e-1),
);

my %by_name = map { $_->{name} => $_ } @items;
my $grand = sum(map { $_->total } @items);

foreach my $item (sort { $a->{name} cmp $b->{name} } @items) {
    next unless $item->{count} > 0;
    printf "%-10s %5d\t%8.2f\n", $item->{name}, $item->{count}, $item->total;
}

if ($grand > 100 and exists $by_name{widget}) {
    print "Grand total: $grand\n";
} elsif (!@items) {
    warn "No items\n";
} else {
    print 'Small order', "\n";
}

my $line = "id=42; name=\"widget\"";
if ($line =~ /^id=(\d+);\s+name="([^"]+)"$/x) {
    my ($id, $name) = ($1, $2);
    (my $upper = $name) =~ s/(\w+)/\U$1/g;
    print "$id: $upper\n";
}

my $count = 0;
while ($count < 3) {
    $count++;
    last if $count == max(2, 5);
}

my $query = <<SQL;
SELECT name, count FROM items WHERE price > 1.5 ORDER BY name;
SQL

eval {
    die "bad price\n" if $items[0]->{price} < 0;
    1;
} or do {
    my $err = $@ || 'unknown error';
    warn "Error: $err";
};

open(my $fh, '<', $0) or die "Cannot open $0: $!";
my @lines = <$fh>;
close $fh;
print scalar(@lines), " lines read\n";

=head1 NAME

sample - an inventory report

=cut

__END__
widget,12,2.50
