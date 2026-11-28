#!/usr/bin/env perl
use strict;
use warnings;
use DB_File;
use Encode qw(decode FB_CROAK);
use Fcntl qw(O_RDONLY);
use File::Basename qw(basename);
use IO::Uncompress::Gunzip qw($GunzipError);
use JSON::PP;

# Emits a small term-report projection of one FlyBase website release.
my ($root, $release) = @ARGV;
die "Usage: $0 FLYBASE_SERVER_ROOT FBYYYY_NN > release-terms.json\n"
    unless @ARGV == 2 && $release =~ /\AFB[0-9]{4}_[0-9]{2}\z/;
my $downloads = "$root/data/downloads";
my $current = readlink "$downloads/current";
die "$downloads/current does not identify $release\n"
    unless defined($current) && basename($current) eq $release;

my $gobo = "$root/indices/crossrefs/gobo";
my $term_id = qr/(?:FBbi|CHEBI):[0-9]+/;
my %obo;
for my $file (qw(image.obo.gz slice.chebi.obo.gz)) {
    my $path = "$downloads/$release/ontologies/$file";
    my $stream = IO::Uncompress::Gunzip->new($path, Strict => 1)
        or die "$path: $GunzipError\n";
    my ($id, $is_term);
    while (my $line = <$stream>) {
        if ($line =~ /^\[([^]]+)\]/) {
            $is_term = $1 eq 'Term';
            undef $id;
        }
        next unless $is_term;
        if ($line =~ /^id: ($term_id)\s*$/) {
            $id = $1;
            die "Duplicate OBO term $id\n" if exists $obo{$id};
            $obo{$id} = [];
        } elsif (defined($id) && $line =~ /^alt_id: ($term_id)\s*$/) {
            push @{ $obo{$id} }, $1;
        }
    }
    die "$path: $GunzipError\n" unless $stream->eof && $stream->close;
}

sub open_index {
    my ($name, $rows) = @_;
    my $path = "$gobo/$name.bdbhash";
    tie %$rows, 'DB_File', $path, O_RDONLY, 0, $DB_HASH or die "$path: $!\n";
}

my (%definitions, %decoder, %terms, %term_counts, %aliases, %edge_labels);
open_index('GODefinitions', \%definitions);
open_index('GODecoder', \%decoder);

# GODefinitions also stores rows containing only annotation hit counts.
# Only actual [Term] stanzas represent ontology terms.
while (my ($id, $raw) = each %definitions) {
    next unless $id =~ /\A$term_id\z/ && $raw =~ /^\[Term\]\t/;
    my ($stanza_id, $name) = $raw =~ /^\[Term\]\tid: (\S+)\tname: ([^\t]+)/;
    die "Invalid GOBO term $id\n"
        unless defined($stanza_id) && $stanza_id eq $id && exists $decoder{$id}
            && (split /\t/, $decoder{$id}, 2)[0] eq $name;
    die "GOBO term $id is absent from the release OBO files\n" unless exists $obo{$id};
    my ($prefix) = split /:/, $id;
    my $term = $terms{$id} = {
        name => decode('UTF-8', $name, FB_CROAK),
        definition => undef,
        comment => undef,
        is_obsolete => JSON::PP::false,
        synonyms => [],
        parents => [],
    };
    ++$term_counts{$prefix};
    for my $alias (@{ delete $obo{$id} }) {
        die "Conflicting alternate ID $alias\n" if exists $aliases{$alias};
        $aliases{$alias} = $id;
    }
    my %synonyms;
    for my $tag (split /\t/, decode('UTF-8', $raw, FB_CROAK)) {
        if ($tag =~ /\A(def|synonym): "((?:[^"\\]|\\.)*)"(?: (EXACT|BROAD|NARROW|RELATED)\b)?(?:\s|\z)/) {
            my ($kind, $text, $scope) = ($1, $2, $3);
            # Decode OBO escapes in quoted text after identifying the closing quote.
            $text =~ s/\\(.)/$1 eq 'n' ? "\n" : $1 eq 't' ? "\t" : $1 eq 'W' ? " " : $1/ge;
            if ($kind eq 'def') {
                $term->{definition} = $text;
            } else {
                my $scope_name = defined($scope) ? lc($scope) : undef;
                my $key = join "\0", $text, ($scope_name // '');
                $synonyms{$key} = { name => $text, scope => $scope_name };
            }
        } elsif ($tag =~ /\A(?:def|synonym):/) {
            die "Invalid quoted GOBO field on $id\n";
        } elsif ($tag =~ /\Acomment: (.*)\z/s) {
            $term->{comment} = $1;
        } elsif ($tag =~ /\A(?:[a-z_]+:|relationship: [^ ]+) ($term_id) ! (.*)\z/) {
            $edge_labels{$id}{$1} = $2;
        } elsif ($tag eq 'is_obsolete: true') {
            $term->{is_obsolete} = JSON::PP::true;
        }
    }
    $term->{synonyms} = [map { $synonyms{$_} } sort keys %synonyms];
}
untie %decoder;
untie %definitions;
die "Missing indexed terms: " . join(', ', sort keys %obo) . "\n" if %obo;
die "Missing ontology in GOBO\n" unless $term_counts{FBbi} && $term_counts{CHEBI};
for my $alias (keys %aliases) {
    die "Alternate ID also appears as a term: $alias\n" if exists $terms{$alias};
}

opendir my $index_dir, $gobo or die "$gobo: $!\n";
my @relationships = sort map { /\A([a-z_]+)\.parents\.bdbhash\z/ ? $1 : () } readdir $index_dir;
closedir $index_dir or die "$gobo: $!\n";
die "Missing is_a GOBO index\n" unless grep { $_ eq 'is_a' } @relationships;
my (%parents, %children);
my $edge_count = 0;
for my $relationship (@relationships) {
    my %rows;
    open_index("$relationship.parents", \%rows);
    for my $id (keys %terms) {
        for my $parent (split /\t/, ($rows{$id} // '')) {
            my $label = exists $terms{$parent} ? $terms{$parent}{name} : $edge_labels{$id}{$parent};
            die "Missing GOBO parent label for $parent of $id\n" unless defined $label;
            push @{ $terms{$id}{parents} }, {
                relationship => $relationship, term_id => $parent, name => $label,
            };
            ++$edge_count;
            $parents{$id}{$parent} = 1;
            $children{$parent}{$id} = 1;
        }
    }
    untie %rows;
}

# Prove the typed projection includes every edge found in the website's untyped indexes.
for my $direction ([parents => \%parents], [children => \%children]) {
    my ($name, $edges) = @$direction;
    my %rows;
    open_index($name, \%rows);
    for my $id (keys %terms) {
        my @actual = sort split /\t/, ($rows{$id} // '');
        my @expected = sort keys %{ $edges->{$id} // {} };
        die "Inconsistent GOBO $name of $id\n" unless "@actual" eq "@expected";
    }
    untie %rows;
}

binmode STDOUT or die "stdout: $!\n";
print JSON::PP->new->utf8->canonical->encode({
    release => $release,
    term_counts => \%term_counts,
    alias_count => scalar keys %aliases,
    edge_count => $edge_count,
    terms => \%terms,
    aliases => \%aliases,
}), "\n" or die "stdout: $!\n";
close STDOUT or die "stdout: $!\n";
