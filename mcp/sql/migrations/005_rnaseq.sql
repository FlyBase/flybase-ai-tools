-- RNA-seq expression profiles for rnaseq_profile_search and rnaseq_similar_genes (spec 5.2, 005).
-- Datasets: the FlyAtlas2 project (RNA-Seq_Profile_FlyAtlas2_* result libraries) and the four modENCODE
-- mRNA-Seq projects (mE_mRNA_* result libraries). Values are the per-gene RPKM library_featureprop rows.
drop table if exists mcp.rnaseq_matrix;
drop table if exists mcp.rnaseq_dataset;
drop table if exists mcp.rnaseq_expression;
create table mcp.rnaseq_expression as
with ds as (
  select l.library_id, l.uniquename as library_uniquename,
         case when l.name like 'RNA-Seq_Profile_FlyAtlas2_%' then 'FlyAtlas2'
              when proj.name = 'modENCODE_mRNA-Seq_development' then 'modENCODE_development'
              when proj.name = 'modENCODE_mRNA-Seq_tissues' then 'modENCODE_tissues'
              when proj.name = 'modENCODE_mRNA-Seq_treatments' then 'modENCODE_treatments'
              when proj.name = 'modENCODE_mRNA-Seq_cell.B' then 'modENCODE_cell_lines' end as dataset,
         regexp_replace(l.name, '^(RNA-Seq_Profile_FlyAtlas2_|mE_mRNA_)', '') as sample
  from library l
  join cvterm lt on lt.cvterm_id = l.type_id and lt.name = 'result'
  join library_relationship lr on lr.subject_id = l.library_id
  join cvterm rt on rt.cvterm_id = lr.type_id and rt.name = 'belongs_to'
  join library proj on proj.library_id = lr.object_id
  where (proj.name = 'FlyAtlas2' and l.name like 'RNA-Seq_Profile_FlyAtlas2_%')
     or (proj.name in ('modENCODE_mRNA-Seq_development', 'modENCODE_mRNA-Seq_tissues', 'modENCODE_mRNA-Seq_treatments', 'modENCODE_mRNA-Seq_cell.B')
         and l.name like 'mE_mRNA_%')
)
select f.uniquename as gene_id, ds.dataset, ds.sample, ds.library_uniquename as library_id, p.value::real as value, 'RPKM'::text as value_type
from ds
join library_feature lf on lf.library_id = ds.library_id
join feature f on f.feature_id = lf.feature_id and f.is_obsolete = false and f.uniquename like 'FBgn%'
join library_featureprop p on p.library_feature_id = lf.library_feature_id
join cvterm pt on pt.cvterm_id = p.type_id and pt.name = 'RPKM'
where p.value ~ '^[0-9]+(\.[0-9]+)?$';
create index rnaseq_expression_ds_sample on mcp.rnaseq_expression (dataset, sample, value);
create index rnaseq_expression_gene on mcp.rnaseq_expression (gene_id, dataset);
analyze mcp.rnaseq_expression;
create table mcp.rnaseq_dataset as
with agg as (
  select dataset, array_agg(distinct sample order by sample) as samples, count(distinct gene_id)::int as gene_count, min(value_type) as value_type
  from mcp.rnaseq_expression group by dataset
),
proj as (
  select * from (values ('FlyAtlas2', 'FlyAtlas2'), ('modENCODE_development', 'modENCODE_mRNA-Seq_development'),
                        ('modENCODE_tissues', 'modENCODE_mRNA-Seq_tissues'), ('modENCODE_treatments', 'modENCODE_mRNA-Seq_treatments'),
                        ('modENCODE_cell_lines', 'modENCODE_mRNA-Seq_cell.B')) v(dataset, project_name)
)
select a.dataset, l.uniquename as project_id, p.project_name, a.samples, a.gene_count, a.value_type
from agg a join proj p using (dataset) join library l on l.name = p.project_name;
create unique index rnaseq_dataset_pk on mcp.rnaseq_dataset (dataset);
create table mcp.rnaseq_matrix as
with samples as (select dataset, count(distinct sample) as n from mcp.rnaseq_expression group by dataset)
select e.gene_id, e.dataset, array_agg(e.value order by e.sample)::real[] as vector,
       avg(e.value)::real as mean, stddev_pop(e.value)::real as stddev
from mcp.rnaseq_expression e join samples s on s.dataset = e.dataset
group by e.gene_id, e.dataset, s.n
having count(*) = s.n;
create unique index rnaseq_matrix_pk on mcp.rnaseq_matrix (dataset, gene_id);
analyze mcp.rnaseq_dataset;
analyze mcp.rnaseq_matrix;
