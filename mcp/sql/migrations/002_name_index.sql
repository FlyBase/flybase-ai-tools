drop table if exists mcp.name_index;
create table mcp.name_index as
with classes as (
  select f.feature_id, f.uniquename as entity_id, o.abbreviation as species,
         case left(f.uniquename, 4)
           when 'FBgn' then 'gene' when 'FBal' then 'allele' when 'FBti' then 'insertion' when 'FBtp' then 'construct'
           when 'FBab' then 'aberration' when 'FBba' then 'balancer' when 'FBog' then 'ortholog' when 'FBco' then 'split_system_combination' end as data_class
  from feature f join organism o using (organism_id)
  where f.is_obsolete = false and left(f.uniquename, 4) in ('FBgn','FBal','FBti','FBtp','FBab','FBba','FBog','FBco')
)
select distinct c.entity_id, c.data_class, c.species, s.name, st.name as name_kind, fs.is_current
from classes c
join feature_synonym fs on fs.feature_id = c.feature_id and fs.is_internal = false
join synonym s on s.synonym_id = fs.synonym_id
join cvterm st on st.cvterm_id = s.type_id
union
select distinct g.uniquename, 'gene_group', 'Dmel', s.name, st.name, gs.is_current
from grp g join grp_synonym gs on gs.grp_id = g.grp_id join synonym s on s.synonym_id = gs.synonym_id join cvterm st on st.cvterm_id = s.type_id
where g.is_obsolete = false
union
select distinct h.uniquename, 'disease_model', 'Hsap', s.name, st.name, hs.is_current
from humanhealth h join humanhealth_synonym hs on hs.humanhealth_id = h.humanhealth_id join synonym s on s.synonym_id = hs.synonym_id join cvterm st on st.cvterm_id = s.type_id
where h.is_obsolete = false
union
select distinct st.uniquename, 'stock', o.abbreviation, g.uniquename, 'genotype', true
from stock st join organism o on o.organism_id = st.organism_id
join stock_genotype sg on sg.stock_id = st.stock_id join genotype g on g.genotype_id = sg.genotype_id
where st.is_obsolete = false
union
-- The stock centre's own number (Bloomington 28813, Kyoto 11010-0031.00).
select distinct st.uniquename, 'stock', o.abbreviation, st.name, 'stock_number', true
from stock st join organism o on o.organism_id = st.organism_id
where st.is_obsolete = false and st.name is not null
union
select distinct c.uniquename, 'cell_line', o.abbreviation, s.name, st.name, cs.is_current
from cell_line c join organism o on o.organism_id = c.organism_id
join cell_line_synonym cs on cs.cell_line_id = c.cell_line_id and cs.is_internal = false
join synonym s on s.synonym_id = cs.synonym_id join cvterm st on st.cvterm_id = s.type_id
where c.is_obsolete = false
union
select distinct l.uniquename, 'library', o.abbreviation, s.name, st.name, ls.is_current
from library l join organism o on o.organism_id = l.organism_id
join library_synonym ls on ls.library_id = l.library_id and ls.is_internal = false
join synonym s on s.synonym_id = ls.synonym_id join cvterm st on st.cvterm_id = s.type_id
where l.is_obsolete = false
union
select distinct sn.uniquename, 'strain', o.abbreviation, s.name, st.name, ss.is_current
from strain sn join organism o on o.organism_id = sn.organism_id
join strain_synonym ss on ss.strain_id = sn.strain_id and ss.is_internal = false
join synonym s on s.synonym_id = ss.synonym_id join cvterm st on st.cvterm_id = s.type_id
where sn.is_obsolete = false;
create index name_index_trgm on mcp.name_index using gin (name gin_trgm_ops);
create index name_index_lower on mcp.name_index (lower(name) text_pattern_ops);
create index name_index_entity on mcp.name_index (entity_id);
analyze mcp.name_index;
